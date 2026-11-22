#!/usr/bin/env python3
"""Finds user-visible text in the UI that doesn't go through translation (M0-09).

Every string a person can read must come from `t!("message-id")` (see crates/loss-i18n). This
scans the UI sources for string literals that look like words and aren't translation ids, CSS
classes, icon names, keys or log lines. Exit code 1 lists each one as file:line.

    python3 scripts/check-i18n.py          # check
    python3 scripts/check-i18n.py --count  # just count

A literal that isn't user text but trips the check can be marked with `// i18n: ignore` on its
line.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# icons.rs holds SVG paths and theme.rs CSS colours: no text.
FILES = [
    f
    for f in sorted((ROOT / "app/src/ui").glob("*.rs"))
    if f.name not in ("icons.rs", "theme.rs")
] + [ROOT / "app/src/notify.rs"]

STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')
# Contexts on the same line, before the literal, where strings are never shown to people.
NOT_TEXT = re.compile(
    r"(class:|style:|key:|\bid:|r#type:|href:|src:|log::|eval\(|#\[|assert|panic!|expect\(|"
    r"unwrap_or_else|name:\s*$|name:\s*\"|rows:|accept:|inputmode:|autocomplete:|"
    r"\bmode\b|getElementById|querySelector|dioxus\.send|\.starts_with\(|\.ends_with\(|"
    r"\.contains\(|split|trim_|strip_|replace\(|join\(|\bext\b|extension|mime|content_type|"
    r"\.join\(\"|matches!|=>\s*\"[a-z0-9_]+\"\s*,|\bt!\(|\btr\(|\btr_args\(|navigator\.|addEventListener)"
)
# Literals that look like ids, classes, icon names, units or punctuation.
IDENT = re.compile(r"^[a-z0-9_.:#/%-]*$")


def letters_outside_placeholders(s: str) -> bool:
    bare = re.sub(r"\{[^}]*\}", "", s)
    bare = re.sub(r"\\[nt\"\\]", "", bare)
    return bool(re.search(r"[A-Za-z]", bare))


def scan(path: Path):
    found = []
    in_tests = False
    for no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("#[cfg(test)]"):
            in_tests = True
        if in_tests:
            continue
        if stripped.startswith("//") or "i18n: ignore" in line:
            continue
        code = line.split("//", 1)[0] if '"' not in line.split("//", 1)[-1] else line
        for m in STRING.finditer(code):
            text = m.group(1)
            bare = re.sub(r"\{[^}]*\}", "", text).strip()
            # A word next to a placeholder ("{} songs") is text; "now-playing{}" is a CSS class.
            formatted_words = "{" in text and re.fullmatch(r"[a-z ]*[a-z][a-z ]*", bare) is not None
            if not letters_outside_placeholders(text) or (IDENT.match(bare) and not formatted_words):
                continue
            before = code[: m.start()]
            # Only the part of the line after the last comma or brace decides the context,
            # so `div { class: "x", "Text" }` still reports "Text".
            last = max(before.rfind(","), before.rfind("{"), before.rfind("("))
            if NOT_TEXT.search(before[last + 1 :] if last >= 0 else before):
                continue
            if NOT_TEXT.search(before) and not re.search(r"[{,]\s*$", before):
                continue
            # Inline CSS declarations ("transform: translateX({x}px); transition: none").
            if re.fullmatch(r"(\s*[a-z-]+\s*:\s*[^;:]+;?)+\s*", text):
                continue
            # CSS class lists inside `class: if … { "a b" } else { "c" }`.
            if "class:" in before and re.fullmatch(r"[a-z0-9 _-]+", text):
                continue
            found.append((path.relative_to(ROOT), no, text))
    return found


FTL = ROOT / "crates/loss-i18n/locales/en/loss.ftl"
USE = re.compile(r'\b(?:t!|tr|tr_args|tr_opt)\(\s*"([a-zA-Z][a-zA-Z0-9_-]*)"')
# Ids built at run time by the settings registry.
DYNAMIC = re.compile(r"^(setting|section|choice)-")


def missing_ids():
    """t!("id") calls whose id isn't in the English file, and English ids nothing uses."""
    defined = set(re.findall(r"^([a-zA-Z][a-zA-Z0-9_-]*)\s*=", FTL.read_text(encoding="utf-8"), re.M))
    used = {}
    for f in list((ROOT / "app/src").rglob("*.rs")) + list((ROOT / "crates").rglob("*.rs")):
        for no, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
            if line.lstrip().startswith("//"):
                continue
            for m in USE.finditer(line):
                used.setdefault(m.group(1), f"{f.relative_to(ROOT)}:{no}")
    missing = sorted((where, i) for i, where in used.items() if i not in defined and i != "no-such-message")
    unused = sorted(i for i in defined if i not in used and not DYNAMIC.match(i))
    return missing, unused


def main():
    hits = [h for f in FILES for h in scan(f)]
    missing, unused = missing_ids()
    for where, i in missing:
        print(f"{where}: t!(\"{i}\") is not in {FTL.relative_to(ROOT)}")
    if unused:
        print(f"Unused messages (remove them or use them): {', '.join(unused)}")
    if missing or unused:
        return 1
    if "--count" in sys.argv:
        print(len(hits))
        return 0
    for f, no, text in hits:
        print(f"{f}:{no}: \"{text}\"")
    if hits:
        print(f"\n{len(hits)} strings are not translated (use t!(\"id\") and add them to "
              f"crates/loss-i18n/locales/en/loss.ftl).", file=sys.stderr)
        return 1
    print("All UI text goes through translation.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
