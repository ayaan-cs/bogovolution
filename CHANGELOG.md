# Changelog

All notable changes, newest first. Dates are in America/Chicago. Nothing has
been published: every version below exists only as a local git commit.

## Unreleased (after 2.7.2)

- Claude runs 3 and 4 (`examples/claude/haiku-5-5-run3/`,
  `examples/claude/sonnet-5-5-run4/`), $0.9766 in all. 28 more-itertools
  tasks chosen from root calibration only. Haiku: root train 67% and held
  out 31%, best accepted rule (a hand written general control) 92% and 50%;
  5 evolve generations reached 100% on train but never moved held out.
  Sonnet 5.5, one run each: root 92% and 44%, same rule 92% and 56%. The
  remaining held out failures need behavior the docstrings do not state.
- `--api-thinking adaptive|off|between_tools` (the last for
  `claude-sonnet-5-5`, which refuses `disabled`) and `--api-effort LEVEL` for Anthropic.
  In Claude run 2, 28 of 240 task replies hit the 2,048 token cap, 14 of
  them with nothing but thinking, so `windowed` never got an answer. Thinking
  off gives the whole cap to the answer. Bad combinations (off at `xhigh` or
  `max`, either flag for OpenAI) are refused before any call. Transcript
  lines now say `stop_reason`, `thinking` and `effort`.
- A bare path line right before a fence (`solve/x.py` then a code block) now
  counts as a file. Claude answered so in 7 of 383 task calls of runs 1 and 2,
  and those answers were scored as "wrote no files".
- `examples/starter/`: a "use it on your own repo" starter. A commented
  `tasks.toml` template, `STARTER.md` (tasks from existing tests and past
  fixes, why checks must be automatic, held out tasks that can move, running
  cheaply, the output cap and thinking, Windows), a tiny Python sample repo
  with hidden tests, a mock model and `try.sh`, which runs it end to end for
  free (self check, mock evolve with `--total`, replay must match). A Rust
  test checks the template parses and fits the sample.
- Second paid run: `claude-haiku-5-5`, 6 train and 10 held out tasks, the
  run 1 rule as a control child plus 3 generations, $0.1417. The run 1 rule
  does not generalize (held out 40% to 20%). Log, replay, transcript, cost
  and findings (2,048 token cap and thinking, saturated held out tasks) in
  `examples/claude/haiku-5-5-run2/`.
- `evolve --total`: `--generations` counts generations already in the log,
  so rerunning the same command after a cut off session keeps the planned
  total (Claude run 1 made 4 generations instead of 3 this way). Without the
  flag nothing changes.
- `[mutation] one_shot_agent` (default false): the mutation prompt says the
  agent answers in one reply, runs nothing and cannot read the hidden paths,
  so it stops proposing rules like "run the checks" or "read checks/" (two of
  the four rules of Claude run 1). Off by default so every existing replay
  still matches.
- First paid run: `claude-haiku-5-5` on 9 more-itertools tasks, 4
  generations, 1 noise and 3 overfit, about $0.07. Log, replay, transcript
  and cost in `examples/claude/haiku-5-5-run1/`.
- `--api-no-temperature`: newer Anthropic models refuse `temperature`.
- `--api-transcript` lines now keep the provider's token usage, so the real
  cost can be computed from input and output tokens.
- Replay fix (found by that run): when a run stopped by a model error was
  redone, its calls got new answers but the recorder skipped them because
  the keys were already in the file, so the replay served the discarded
  answers and disagreed with the log. A call made again with a different
  answer is now appended, and on load the later entry for a key wins (a
  repeated key used to be refused). Same answers are still not written
  twice.
- Windows build: `packaging/windows/build.sh` cross compiles
  `rules-evolve.exe` (x86_64-pc-windows-gnu) and packs it with the viewer,
  the replay demo, `demo.ps1` and `WINDOWS-SETUP.md`.
- Windows fixes, no change on Linux: process groups and Unix file owners
  only on Unix; a timed out check is stopped with `taskkill /T /F` on
  Windows; `PATH` lookups use the platform separator and `.exe`; bubblewrap
  reports "runs only on Linux"; Windows system variables such as
  `SystemRoot` are kept for check commands and container runtimes.
- Windows with docker or podman: the drive letter's `:` no longer refuses
  every path (found on the first real Windows run with Docker Desktop).
  On Windows, folders are passed as `--mount type=bind,source=...` instead
  of `--volume`; only the drive's own `:` is allowed, `,` `:` and `"` in the
  rest of the path, network shares and device paths are still refused. The
  message no longer suggests bwrap outside Linux.
- Windows: file paths in the model's repo snapshot use `/`, as on Linux, so
  prompts, replay keys and `context` matching are the same on every OS
  (found by running the unit tests under Wine).
- Unix only tests are marked so, and the core, watch and CLI tests build for
  Windows; the core unit tests and the evolve tests pass under Wine.
- When no isolation backend works, the error says what to install for the
  platform (on Windows: Docker Desktop, Podman Desktop or WSL2). On Windows
  a missing `sh` points to Git for Windows or WSL2.

## 2.7.2 (2026-11-17)

Hardening and tests for the problems the real model runs hit. See
`docs/dev-notes/2026-11-17-evolve-testing.md`.

- Fix: a failed model call (for example the server killed for memory) was
  counted as a failed task, so a dead server gave a fake "rejected" verdict.
  The run now stops with a clear error, is not recorded, and nothing is
  counted; `evolve` run again resumes. `[evaluation] max_model_errors_per_run`
  (default 0) tolerates some failed calls the old way.
- `[mutation] retries_on_refusal` (default 0): `evolve` asks again with the
  next seed after a refused mutation (repeat, whitespace only change, empty,
  unreadable or oversized answer). Token cap refusals are not retried. A
  retry missing from a replay ends the generation with the last refusal.
- New refusal: a change that differs from the parent or a version in the
  tree only in whitespace.
- `evolve` finishes a proposed but unjudged child from an earlier session
  before asking for a new mutation (reported as "resumed").
- Token cap: the run that hits the cap is recorded, no further run of that
  version starts, `run` prints "STOPPED AT THE TOKEN CAP" with the numbers,
  `verify` refuses to judge capped runs, and `evolve` stops with the note.
- Log repair before every write: an incomplete last line moves to
  `log.jsonl.partial`, a missing final newline is added, and a `HEAD` one
  event behind is updated. Tampering is still never repaired.
- 22 new tests: 20 in `crates/core/tests/evolve_hardening.rs` (including a
  positive control: on a deterministic mock a general rule must be accepted
  and a train only rule must be judged overfit) and 2 CLI tests.

## 2.7.1 (2026-11-16)

- Fix: `verify-log` called a valid log edited when a median had a long
  decimal (10 of 24 is 0.4166666666666667). serde_json read such a number
  back as a slightly different float, so the recomputed hash differed, and
  appending to that log was refused. serde_json now uses `float_roundtrip`;
  no log format change, and logs that failed this way now verify. Found when
  the control's verdict on the 48 task set failed `verify-log`.
- 7B rerun on the 48 task set with short prompts (`SHORT_CONTEXT=1`), 5 runs
  for every version: root, control and a 3 generation evolve. About 16 times
  fewer tokens and about 1.8 times faster per call than with every stub in
  the prompt. Nothing accepted, nothing generalized; the control fixed its
  two target tasks and lost about as many (noise). The server was killed for
  memory 22 calls into generation 3, so that verdict is invalid and not
  counted. Generation 3 was rerun alone with `--cache-ram 1024`: rejected
  (-4.2, 1 of 5; held out 0.0, 2 of 5). Replays in
  `replays/qwen2.5-coder-7b/oss48-short/` (`evolve-gen3-replay.jsonl` is
  the rerun).
- `replay.sh oss48` skips the `islice` stage when the folder has no
  `islice-replay.jsonl`, and `NEXT_REPLAY` replays one more generation
  recorded later on top of the first `GENERATIONS`.

## 2.7.0 (2026-11-16)

- `context` per task in `tasks.toml`: the files (or folders ending in `/`)
  the model is shown for that task. Without it the prompt and the replay key
  are exactly as before, so older recordings replay unchanged; with it the
  list is part of the replay key. `make-oss-fixture.py --short-context` and
  `SHORT_CONTEXT=1` for `run-oss48.sh` and `replay.sh oss48` use it.
- Held out gate for large sets: with at least `held_out_large_set_tasks`
  (default 20) held out tasks, the held out gain needed is
  `held_out_large_set_threshold_pp` (default 5) instead of
  `held_out_accept_threshold_pp` (10). 3 of 5 wins still needed. Configs
  without the keys get the defaults. `verify` and the export show the bar
  that applied.
- `run-oss48.sh` runs the islice stage last, after the evolve.
- A 48 task more-itertools set (24 train, 24 held out; the 16 earlier tasks
  keep their splits). `make-oss-fixture.py` reads `recipes.py` and `more.py`,
  `validate-oss-fixture.sh` checks each task passes on upstream code and
  fails on the stub, `oss48-tasks.sh` lists the set.
- `run-oss48.sh`: root, control, the old `islice` rule and a combined
  `evolve` as restartable stages with done markers, and `LATER_REPEATS` to
  run the later stages with fewer runs. `replay.sh oss48` replays it.
- 7B results on the 48 task set (root 5 runs, the rest 2 runs): nothing
  accepted, nothing generalized. Replays in `replays/qwen2.5-coder-7b/oss48/`.
- `verdict-noise.py`, which simulates the verdict rule from a root's
  measured runs; it is the basis of the large set held out bar above.
- README: install from source, a one minute quickstart with the replay demo,
  and a short summary of what the real model runs found.
- `docs/writeup.md`: the story and the numbers of the real model runs.
- Combined 7B `evolve` with 8 train and 8 held out tasks: nothing accepted,
  2 rejected, 1 refused; the model never proposed the narrow `islice` rule this
  time. Replay committed as `oss-held-out-replay.jsonl`.
- Fix: failure notes kept test run times (`Ran 3 tests in 0.002s`), which
  change on every run, so a replay of a run with failure notes could miss on
  a mutation. Run times are now dropped from the notes, and a replayed
  mutation whose exact key misses is matched by model, parent version and
  seed when exactly one recorded mutation fits. Task calls still match
  exactly. All committed 7B replays reproduce again.
- `examples/local-model/replay.sh` makes replay file paths absolute, so the
  documented relative paths work.
- `docs/release-checklist.md`: a draft checklist for a first public release.
- The crate version now matches the documented version (2.6.0, was 0.1.0), so
  `rules-evolve --version` and the `generator.version` field of an export say
  2.6.0. The bundled demo export was regenerated for it.

## 2.6.0 (2026-11-13)

- `docker` and `podman` ran the full escape and hidden file test suites for
  the first time (Docker 29.9.3 rootful, Podman 6.2.0 rootless) and pass.
- Fix: rootless podman without cgroup delegation silently ignored
  `--pids-limit` and `--memory`. Podman now also gets `--ulimit nproc` and
  `--ulimit data`, which hold without cgroups.

## 2.5.0 (2026-11-13)

- Paid, bring your own key providers: `anthropic` (Messages API) and `openai`
  (any OpenAI compatible chat completions endpoint). They cost money on your
  own key, need `--allow-spend` and `--api-model`, read the key only from an
  environment variable, never write it anywhere, use https (or http to
  localhost only), follow no redirects, and require isolation. Tested only
  against a fake local server.
- The Claude Code plugin wrapper refuses `--allow-spend`.
- New dependency for HTTPS: `ureq` with `rustls`.

## 2.4.0 (2026-11-13)

- `apply`: writes an accepted version into the rules file. A dry run with a
  diff unless `--yes`; versions that were not accepted need `--force`; never
  writes outside the repo or through a symlink; records an `applied` event.
  Logs with an `applied` event cannot be read by older binaries.
- Claude Code plugin 2.1.0: `/rules-evolve:apply`, which shows the diff and
  asks before writing.

## 2.3.0 (2026-11-11)

- Held out tasks: `split = "held_out"` in `tasks.toml`. The mutation prompt
  never shows them. A child that wins on train but not on held out gets the new
  verdict `overfit`, and `current` does not move.
- Held out check of the 7B result: the accepted `islice` rule is overfit
  (37.5% against 50% on 8 held out tasks), while a general hand written rule
  gains on both. Replay committed.

## 2.2.0 (2026-11-11)

- Failure notes: the mutation prompt includes short notes on why tasks failed.
- Local model test with Qwen2.5 Coder 7B Instruct (Apache 2.0): one mutation
  accepted, +25 points, 5 of 5 paired wins, in two independent runs. Replays
  committed.

## 2.1.0 (2026-11-11)

- `local` provider: a free model server on this machine (llama.cpp or any
  OpenAI compatible server on loopback). Real model output, so isolation is
  required.
- First real model test (Qwen2.5 Coder 3B): nothing accepted. Its answers are
  not shipped because of the model's license.

## 2.0.0 (2026-11-11)

- Claude Code plugin with slash commands and opt in hooks. Mock and replay
  providers only.

## 1.2.4 and earlier (2026-11-11)

- 1.2.4: readable lineage graph in the viewer.
- 1.2.3: viewer chart and `watch` polish.
- 1.2.2: shadow warnings for layouts where model files could shadow hidden
  tests.
- 1.2.1: hidden test files mounted read only during the check.
- 1.2: pluggable isolation for the check command (`bwrap`, `docker`,
  `podman`, `none`, `auto`), and the rule that a real provider must run
  isolated.
- 1.1: the `watch` terminal view and replay `--pace`.
- Phases 1 to 4: the hash chained log, the benchmark harness and verdict rule,
  `mutate` and `evolve` with record and replay, `digest`, `export` with a
  versioned JSON schema, and the static viewer.

Inspired by [Petri](https://ethglobal.com/showcase/petri-mjy1y) (ETHOnline
2026).
