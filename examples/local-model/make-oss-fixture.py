#!/usr/bin/env python3
"""Build a small coding task repo from the real more-itertools project.

For each chosen function of more_itertools/recipes.py or more_itertools/more.py
this writes

  solve/<name>.py          a stub: the upstream signature and docstring, no body
  checks/test_<name>.py    the upstream unit tests for that function, unchanged
                           except that they import the stub instead of the package

The checks are hidden from the model by bogo. This is a task set DERIVED
from a real open source project (MIT licensed), not the whole project.

  make-oss-fixture.py <more-itertools checkout> <destination> [fn,fn,...]
                     [--held-out=fn,fn,...] [--reference] [--tasks-out=PATH]
                     [--short-context]

--held-out adds more functions as held out tasks (split = "held_out" in
tasks.toml): bogo runs and scores them, but never shows them to the
model that writes the rules, and accepts a change only if it also gains on them.

--short-context gives every task `context = ["solve/<name>.py"]`, so the model
is shown only that task's stub instead of every stub (bogo 2.7.0 and
later). Without it the task file is the same as before.

--tasks-out writes tasks.toml somewhere else (default: <destination>/tasks.toml).
Put it in <destination>/.bogo/ after `bogo init`, so the model
never sees the check commands.

--reference fills each stub file with the whole upstream module, to
prove the checks pass for a correct answer. Without it every check must fail
on the stub. Reference output is for local validation only; the stubs hold
only signatures and docstrings, never an implementation.
"""
import ast
import sys
from pathlib import Path

# function name -> upstream test class in tests/test_recipes.py.
# Every entry is validated: the check passes on the upstream code, fails on the
# stub, and the test touches no other more_itertools name.
FUNCS = {
    "take": "TakeTests",
    "tail": "TailTests",
    "nth": "NthTests",
    "quantify": "QuantifyTests",
    "ncycles": "NcyclesTests",
    "dotproduct": "DotproductTests",
    "flatten": "FlattenTests",
    "repeatfunc": "RepeatfuncTests",
    "first_true": "FirstTrueTests",
    "iter_except": "IterExceptTests",
    "unique_everseen": "UniqueEverseenTests",
    "unique_justseen": "UniqueJustseenTests",
    "powerset": "PowersetTests",
    "prepend": "PrependTests",
    "roundrobin": "RoundrobinTests",
    "partition": "PartitionTests",
    "grouper": "GrouperTests",
    "consume": "ConsumeTests",
    "tabulate": "TabulateTests",
    "nth_combination": "NthCombinationTests",
    "before_and_after": "BeforeAndAfterTests",
    "triplewise": "TriplewiseTests",
    "subslices": "SubslicesTests",
    "iter_index": "IterIndexTests",
    "transpose": "TransposeTests",
    "unique": "UniqueTests",
    "random_product": "RandomProductTests",
    "random_permutation": "RandomPermutationTests",
    "random_combination": "RandomCombinationTests",
    "random_combination_with_replacement": "RandomCombinationWithReplacementTests",
    "sliding_window": "SlidingWindowTests",
    "polynomial_from_roots": "PolynomialFromRootsTests",
    "reshape": "ReshapeTests",
    "matmul": "MatMulTests",
    "polynomial_eval": "PolynomialEvalTests",
    "sum_of_squares": "SumOfSquaresTests",
    "polynomial_derivative": "PolynomialDerivativeTests",
    "totient": "TotientTests",
    "loops": "LoopsTests",
}

# function name -> upstream test class in tests/test_more.py (same rules).
MORE_FUNCS = {
    "ilen": "IlenTests",
    "intersperse": "IntersperseTest",
    "windowed": "WindowedTests",
    "split_at": "SplitAtTests",
    "chunked": "ChunkedTests",
    "distribute": "DistributeTest",
    "interleave_longest": "InterleaveLongestTests",
    "mark_ends": "MarkEndsTests",
    "locate": "LocateTests",
    "consecutive_groups": "ConsecutiveGroupsTest",
    "duplicates_justseen": "DuplicatesJustSeenTests",
    "longest_common_prefix": "LongestCommonPrefixTests",
    "circular_shifts": "CircularShiftsTests",
    "unique_to_each": "UniqueToEachTests",
    "zip_offset": "ZipOffsetTest",
    "sort_together": "SortTogetherTest",
    "chunked_even": "ChunkedEvenTests",
    "split_when": "SplitWhenTests",
    "stagger": "StaggerTest",
    "exactly_n": "ExactlyNTests",
}

RULES = "# Project rules\n\nWrite clear code and keep changes small.\n"


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    reference = "--reference" in sys.argv
    short_context = "--short-context" in sys.argv
    tasks_out = next(
        (a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--tasks-out=")),
        None,
    )
    src_dir, dest = Path(args[0]), Path(args[1])
    names = args[2].split(",") if len(args) > 2 else list(FUNCS) + list(MORE_FUNCS)
    held_out = next(
        (a.split("=", 1)[1].split(",") for a in sys.argv[1:] if a.startswith("--held-out=")),
        [],
    )
    held_out = [h for h in held_out if h]
    overlap = sorted(set(names) & set(held_out))
    if overlap:
        sys.exit(f"a function cannot be both train and held out: {', '.join(overlap)}")
    recipes = (src_dir / "more_itertools/recipes.py").read_text()
    more = (src_dir / "more_itertools/more.py").read_text()
    # more.py imports recipes relatively; the reference copy imports it from solve/
    more_ref = more.replace("from .recipes import", "from solve._recipes_ref import")
    upstream = {}
    for mod, code, test_file, table in (
        ("recipes", recipes, "tests/test_recipes.py", FUNCS),
        ("more", more, "tests/test_more.py", MORE_FUNCS),
    ):
        tests = (src_dir / test_file).read_text()
        mt, tt = ast.parse(code), ast.parse(tests)
        upstream[mod] = {
            "code": code,
            "tests": tests,
            "table": table,
            "funcs": {n.name: n for n in mt.body if isinstance(n, ast.FunctionDef)},
            "classes": {n.name: n for n in tt.body if isinstance(n, ast.ClassDef)},
            "t_imports": [
                ast.get_source_segment(tests, n)
                for n in tt.body
                if isinstance(n, (ast.Import, ast.ImportFrom))
            ],
        }
    (dest / "solve").mkdir(parents=True, exist_ok=True)
    (dest / "checks").mkdir(parents=True, exist_ok=True)
    (dest / "solve/__init__.py").write_text("")
    (dest / "checks/__init__.py").write_text("")
    (dest / "AGENTS.md").write_text(RULES)
    if reference:
        (dest / "solve/_recipes_ref.py").write_text(recipes)
    for name in names + held_out:
        u = upstream["recipes" if name in FUNCS else "more"]
        code, tests = u["code"], u["tests"]
        f = u["funcs"][name]
        lines = code.splitlines()
        first = (f.decorator_list[0].lineno if f.decorator_list else f.lineno) - 1
        if reference:
            # The whole upstream module, so helpers the function relies on
            # (`_sumprod`, `batched`, other recipes) are there. Only used to
            # prove the checks pass for a correct answer; never committed.
            ref = recipes if name in FUNCS else more_ref
            (dest / f"solve/{name}.py").write_text(ref)
        else:
            sig = "\n".join(lines[first : f.body[0].lineno - 1])
            doc = ast.get_docstring(f, clean=False)
            has_doc = (
                isinstance(f.body[0], ast.Expr)
                and isinstance(getattr(f.body[0], "value", None), ast.Constant)
                and isinstance(f.body[0].value.value, str)
            )
            if has_doc:
                doc_src = "\n".join(lines[f.body[0].lineno - 1 : f.body[0].end_lineno])
                stub = f"{sig}\n{doc_src}\n    raise NotImplementedError\n"
            else:
                stub = f"{sig}\n    raise NotImplementedError\n"
            (dest / f"solve/{name}.py").write_text(stub)
        c = u["classes"][u["table"][name]]
        cfirst = (c.decorator_list[0].lineno if c.decorator_list else c.lineno) - 1
        csrc = "\n".join(tests.splitlines()[cfirst : c.end_lineno])
        header = "\n".join(
            f"import solve.{name} as mi" if "more_itertools as mi" in i else i
            for i in u["t_imports"]
            if "more_itertools" not in i or "as mi" in i
        )
        (dest / f"checks/test_{name}.py").write_text(header + "\n\n\n" + csrc + "\n")
    # tasks.toml
    out = [
        "# Tasks derived from more-itertools (MIT). Each task: implement one function from its docstring.",
        "# The upstream unit tests live in checks/ and are hidden from the model.",
        "",
        "[sandbox]",
        'hidden = ["checks"]',
        "",
    ]
    for name in names + held_out:
        out += [
            "[[task]]",
            f'id = "{name}"',
            f'prompt = "Implement the function `{name}` in solve/{name}.py. Keep the signature and follow the docstring exactly. Write the whole file."',
            f'check = "python3 -B -m unittest checks.test_{name}"',
            "timeout_secs = 60",
        ]
        if short_context:
            out.append(f'context = ["solve/{name}.py"]')
        if name in held_out:
            out.append('split = "held_out"')
        out.append("")
    Path(tasks_out or dest / "tasks.toml").write_text("\n".join(out))


main()
