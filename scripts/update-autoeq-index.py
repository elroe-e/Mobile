#!/usr/bin/env python3
"""Rebuilds crates/loss-core/data/autoeq-index.tsv from AutoEq's results index (MIT licence,
github.com/jaakkopasanen/AutoEq). One line per profile: name, source, rig, path (URL-encoded,
relative to AutoEq's results folder). Run when AutoEq adds headphones:

    python3 scripts/update-autoeq-index.py            # downloads INDEX.md
    python3 scripts/update-autoeq-index.py INDEX.md   # or reads a saved copy
"""
import re
import sys
import urllib.request
from pathlib import Path

URL = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/INDEX.md"
OUT = Path(__file__).resolve().parent.parent / "crates/loss-core/data/autoeq-index.tsv"

if len(sys.argv) > 1:
    text = Path(sys.argv[1]).read_text(encoding="utf-8")
else:
    req = urllib.request.Request(URL, headers={"User-Agent": "Loss-build"})
    text = urllib.request.urlopen(req, timeout=60).read().decode("utf-8")

# "by <source> on <rig>"; the rig is missing for older sources.
LINE = re.compile(r"^- \[(.+)\]\(\./(.+)\) by (.+?)(?: on (.+))?$")
rows = []
for line in text.splitlines():
    m = LINE.match(line.strip())
    if not m:
        continue
    name, path, source, rig = ((s or "").replace("\t", " ").strip() for s in m.groups())
    rows.append(f"{name}\t{source}\t{rig}\t{path}")
OUT.write_text("\n".join(rows) + "\n", encoding="utf-8")
print(f"{OUT}: {len(rows)} profiles, {OUT.stat().st_size // 1024} KB")
