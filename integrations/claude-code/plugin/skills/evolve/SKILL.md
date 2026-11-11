---
name: evolve
description: Run the rules-evolve loop (mutate, run, verify) for a few generations on the free mock model or a replay file. Appends to the lineage log. Never calls a real model.
argument-hint: "[generations] [--seed N] [--replay-file PATH]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Run `evolve` for the repo in the current directory. This costs nothing: only the built in mock model or a recorded replay file is used, and the plugin refuses any other provider.

Arguments: `$ARGUMENTS`

Work out the options:

- generations: the first number in the arguments, default 3 (the CLI accepts 1 to 100).
- seed: `--seed N` if given, otherwise 0.
- replay: if `--replay-file PATH` is given, use `--provider replay --replay-file PATH`. Otherwise use `--provider mock`. Never pass any other provider.

Run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh evolve --generations <n> --seed <seed> --provider <mock|replay> [--replay-file <path>] --json
```

This appends events to `.rules-evolve/log.jsonl` (append only, hash chained). Tell the user that before or right after running it.

Common errors to explain plainly: mock settings missing (`.rules-evolve/mock.toml`, see the README "Mock model" section, or copy `examples/demo/mock.toml`), a replay miss (the replay file does not hold that seed or generation count, which is intended), or tasks not set up (`.rules-evolve/tasks.toml`). Do not create or edit those files without the user asking.

On success summarize: generations requested, accepted, rejected, noise and refused counts, the current version before (`start`) and after (`end`) as 12 character short hashes, and total tokens (mock tokens, no money). Then a compact list per generation: verdict and the hypothesis. If a version was accepted, say that it is stored in `.rules-evolve/blobs/<full hash>`, and that the plugin does not copy it into the real rules file: that stays the user's decision.

Do not call any model other than the mock or replay provider.
