"""Self check for the honest example, free and offline.

For every task in tasks.toml:
  1. the stub as shipped in repo/kit/ fails its hidden check,
  2. the reference solution in reference/kit/ passes it,
  3. the examples in the stub's docstring pass against the reference
     (doctest), so the docstring and the checks agree.
It also checks that every task has a context file, a check file and a
reference, and that repo/ holds no reference code.

    python3 examples/honest/selfcheck.py

Needs Python 3.11 or later (tomllib).
"""
import doctest
import importlib.util
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ImportError:
    print("selfcheck skipped: needs Python 3.11 or later (tomllib)")
    sys.exit(0)

here = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).parent)
repo = here / "repo"
tasks = tomllib.loads((here / "tasks.toml").read_text(encoding="utf-8"))["task"]
bad = []


def check(tree, t):
    cmd = t["check"].replace("python3", sys.executable, 1)
    r = subprocess.run(cmd, shell=True, cwd=tree, capture_output=True, timeout=t.get("timeout_secs", 60))
    return r.returncode == 0


splits = {"train": 0, "held_out": 0}
for t in tasks:
    tid = t["id"]
    splits[t.get("split", "train")] += 1
    ctx = t["context"][0]
    ref = here / "reference" / ctx
    missing = [p for p in [repo / ctx, repo / "checks" / f"test_{tid}.py", ref] if not p.exists()]
    if missing:
        bad.append(f"{tid}: missing {missing}")
        continue
    stub = (repo / ctx).read_text(encoding="utf-8")
    if "raise NotImplementedError" not in stub:
        bad.append(f"{tid}: the shipped file is not a stub")
    with tempfile.TemporaryDirectory() as d:
        shutil.copytree(repo, d, dirs_exist_ok=True)
        stub_passes = check(d, t)
        shutil.copy(ref, Path(d) / ctx)
        ref_passes = check(d, t)
        # The docstring examples, run against the reference code.
        spec = importlib.util.spec_from_file_location(f"ref_{tid}", ref)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        fn_name = t["prompt"].split("`")[1]
        doc_holder = {}
        exec(compile(stub.replace("raise NotImplementedError", "pass"), ctx, "exec"), doc_holder)
        docstring = doc_holder[fn_name].__doc__
        finder = doctest.DocTestParser()
        test = finder.get_doctest(docstring, {fn_name: getattr(mod, fn_name)}, tid, ctx, 0)
        runner = doctest.DocTestRunner(optionflags=doctest.ELLIPSIS)
        runner.run(test, out=lambda s: None)
        doc_ok = runner.failures == 0 and runner.tries > 0
    status = "ok" if (not stub_passes and ref_passes and doc_ok) else "WRONG"
    print(f"  {tid:<18} stub {'passes' if stub_passes else 'fails '}  reference {'passes' if ref_passes else 'fails '}"
          f"  docstring examples {runner.tries - runner.failures}/{runner.tries}  {status}")
    if stub_passes:
        bad.append(f"{tid}: the stub passes its check")
    if not ref_passes:
        bad.append(f"{tid}: the reference fails its check")
    if not doc_ok:
        bad.append(f"{tid}: docstring examples disagree with the reference")
leaks = [p for p in repo.rglob("*") if p.is_dir() and p.name == "reference"]
if leaks:
    bad.append(f"reference code inside repo/: {leaks}")
if bad:
    sys.exit("selfcheck FAILED:\n  " + "\n  ".join(bad))
print(f"selfcheck OK: {len(tasks)} tasks ({splits['train']} train, {splits['held_out']} held out)")
