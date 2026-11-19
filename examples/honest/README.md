# The honest example: 24 tasks where the docstring is the whole spec

In the more-itertools runs (`../claude/`), the last held out failures needed
behavior the docstrings never state (an exact error message, raising for
`steps=0`). No honest rule can teach that. This example removes that gap:
every behavior a hidden check tests (results, edge cases, error types) is
written in the docstring of the stub the model fills in.

- `repo/`: what the model works in. `kit/<task>.py` is a stub with a full
  docstring and `raise NotImplementedError`. `checks/` holds the unit tests,
  hidden from the model. `AGENTS.md` is the starting rules file.
- `reference/kit/`: one reference solution per task. Never copied into a
  project, never shown to the model.
- `tasks.toml`: 24 tasks, 16 train and 8 held out. The prompt is the same
  for every task: implement the function, keep the signature, follow the
  docstring exactly, write the whole file.
- `selfcheck.py`: for every task the stub fails its check, the reference
  passes it, and the docstring examples pass against the reference (doctest).

```sh
python3 examples/honest/selfcheck.py
```

To run it, copy `repo/` somewhere, `bogo init`, and put
`tasks.toml` at `.bogo/tasks.toml`. The paid Claude run on it is in
`../claude/haiku-5-5-run5/`.

The task set and split were fixed before any model saw them.
