"""Self check for the starter sample: every reference fix in mock.toml
passes its hidden test, and every file as shipped fails it. Run from
try.sh, or as: python3 selfcheck.py <examples/starter folder>.
Also checks that tasks.toml and mock.toml agree (same task ids, context
files exist). Needs Python 3.11 or later for tomllib; older Pythons skip.
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ImportError:  # Python before 3.11
    print("selfcheck skipped: needs Python 3.11 or later (tomllib)")
    sys.exit(0)

here = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).parent)
tasks = tomllib.loads((here / "tasks.toml").read_text(encoding="utf-8"))
mock = tomllib.loads((here / "mock.toml").read_text(encoding="utf-8"))
answers = {a["task"]: a for a in mock["answer"]}
repo = here / "sample-repo"
bad = []
ids = [t["id"] for t in tasks["task"]]
if sorted(ids) != sorted(answers):
    bad.append(f"tasks.toml ids {sorted(ids)} differ from mock.toml {sorted(answers)}")
for t in tasks["task"]:
    for c in t.get("context", []):
        if not (repo / c).exists():
            bad.append(f"{t['id']}: context file {c} is missing")
for h in tasks["sandbox"]["hidden"]:
    if not (repo / h).exists():
        bad.append(f"hidden path {h} is missing")

for t in tasks["task"]:
    a = answers.get(t["id"])
    if a is None:
        continue
    for label, content, want in [("fix", a["pass_content"], 0), ("as shipped", None, 1)]:
        with tempfile.TemporaryDirectory() as d:
            shutil.copytree(repo, d, dirs_exist_ok=True)
            if content is not None:
                (Path(d) / a["path"]).write_text(content, encoding="utf-8")
            else:
                shipped = (repo / a["path"]).read_text(encoding="utf-8")
                if shipped != a["fail_content"]:
                    bad.append(f"{t['id']}: fail_content is not the shipped file")
            cmd = t["check"].replace("python3", sys.executable, 1)
            r = subprocess.run(cmd, shell=True, cwd=d, capture_output=True)
            ok = (r.returncode == 0) == (want == 0)
            print(f"  {t['id']:<11} {label:<10} {'passes' if r.returncode == 0 else 'fails'}"
                  f"{'' if ok else '  <- WRONG'}")
            if not ok:
                bad.append(f"{t['id']}: the {label} file should {'pass' if want == 0 else 'fail'}")
if bad:
    sys.exit("selfcheck FAILED:\n  " + "\n  ".join(bad))
print(f"selfcheck OK: {len(ids)} tasks")
