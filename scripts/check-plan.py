#!/usr/bin/env python3
"""Consistency check for the plan anchor (run at the start and end of every session).

* every task ID referenced in docs/plan/PARITY.md exists in docs/plan/PLAN.md
* per-milestone task and done counts in PLAN.md's Status table match the checkboxes
* prints the next unchecked tasks from the "Next up" list

`--sync` first rewrites PARITY.md states from PLAN.md checkboxes: an item is
`done` when all its tasks are ticked, `doing` when some are, else `todo`
(`keep` and `excluded` rows are left alone).
"""
import re, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
plan = (root / "docs/plan/PLAN.md").read_text()
parity = (root / "docs/plan/PARITY.md").read_text()

tasks = {}
for m in re.finditer(r"^- \[( |x)\] \*\*(M\d+-\d+)", plan, re.M):
    tasks[m.group(2)] = m.group(1) == "x"

if "--sync" in sys.argv:
    def sync_row(m):
        line = m.group(0)
        cells = line.split("|")
        state = cells[-2].strip()
        if state in ("keep", "excluded"):
            return line
        ids = re.findall(r"\bM\d+-\d+\b", cells[-3])
        done = [tasks.get(i, False) for i in ids]
        new = "done" if ids and all(done) else "doing" if any(done) else "todo"
        cells[-2] = f" {new} "
        return "|".join(cells)
    parity = re.sub(r"^\| [A-Z]+-\d+ \|.*\|$", sync_row, parity, flags=re.M)
    (root / "docs/plan/PARITY.md").write_text(parity)

refs = set(re.findall(r"\bM\d+-\d+\b", parity))
missing = sorted(r for r in refs if r not in tasks)
ok = True
if missing:
    ok = False
    print("PARITY.md references tasks that PLAN.md doesn't define:", ", ".join(missing))

for row in re.finditer(r"^\| (M\d+) \| [^|]+\| (\d+) \| (\d+) \|", plan, re.M):
    ms, total, done = row.group(1), int(row.group(2)), int(row.group(3))
    ids = [t for t in tasks if t.startswith(ms + "-")]
    real_done = sum(tasks[t] for t in ids)
    if len(ids) != total or real_done != done:
        ok = False
        print(f"Status table out of date for {ms}: table says {total} tasks / {done} done, "
              f"checkboxes say {len(ids)} / {real_done}")

states = re.findall(r"\| (todo|doing|done|keep|excluded) \|\s*$", parity, re.M)
counts = {s: states.count(s) for s in ("todo", "doing", "done", "keep", "excluded")}
print(f"Tasks: {sum(tasks.values())}/{len(tasks)} done. Parity items: " +
      ", ".join(f"{v} {k}" for k, v in counts.items()))

nxt = plan[plan.index("## Next up"):plan.index("## Open decisions")]
pending = [t for t in re.findall(r"\*\*(M\d+-\d+)\*\*", nxt) if not tasks.get(t, False)]
print("Next up (unchecked):", ", ".join(pending[:6]) if pending else "see milestone order")
sys.exit(0 if ok else 1)
