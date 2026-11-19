# Claude run 2: claude-haiku-5-5, 6 train and 10 held out tasks

The second paid run. Ayaan approved it with a target under $0.25 and a hard
stop at $0.50. It ran on the Linux box under bwrap, in a project outside this
repo, on Nov 17, 2026 from 10:42 PM to 11:00 PM CST. It cost **$0.1417**.

| File | What it is |
| --- | --- |
| `replay.jsonl` | Every model answer (243 calls). `replay.sh` replays the whole run. |
| `replay.sh` | Rebuilds the project from a more-itertools checkout and replays it. No key, no network. |
| `run2.sh` | The script that made the paid run (restartable, `evolve --total`). |
| `log.jsonl` | The run's hash-chained log (24 events, `verify-log` OK). |
| `tree.json` | `bogo export` of the log, for the viewer. |
| `api-transcript.jsonl` | Each API call: reply, seed and the provider's token usage. |
| `rules/` | The five rules files, named by hash prefix. |
| `config.toml` | The project config. |

## What changed before the run

- `evolve --total`: `--generations` counts children already in the log, so a
  rerun after a timeout keeps the planned total (run 1 ended with 4
  generations instead of 3). The run used `--generations 4 --total`: the
  control child below plus 3 generations. No timeout happened this time.
- `[mutation] one_shot_agent = true`: the mutation prompt now says the agent
  answers in one reply, runs nothing and cannot read the hidden `checks`.
  Two of the four rules of run 1 asked the agent to run the checks or read
  `checks/`, which it cannot do. None of the three new rules here does.
  The setting is off by default so older replays still match.

## Setup

- Model `claude-haiku-5-5` ($0.10 per million input tokens, $0.50 per million
  output tokens), `--api-no-temperature --api-max-new-tokens 2048`, seed 7.
- Train (same as run 1): take, nth, distribute, nth_combination,
  sort_together, polynomial_from_roots.
- Held out (10, from the 48 task set): ncycles, consume, windowed,
  intersperse, sliding_window, locate, zip_offset, grouper, transpose,
  circular_shifts.
- `repeats = 3`, `min_paired_wins = 3`, `held_out_min_paired_wins = 2`,
  `max_tokens_per_run = 70000`, isolation bwrap.
- Steps: root (3 runs), then the run 1 generation 1 rule (0ea6c448) added
  with `propose` as a control child of the root and judged, then `evolve` to
  a total of 4 children. The evolve saw the control in its history, so it
  did not spend a generation finding it again.
- Root runs were measured again. The task set differs from run 1 (new held
  out tasks), and a run covers the whole set, so run 1's root runs could not
  be reused.

### How the held out tasks were picked, and what was dropped

Dropped from run 1's held out set: tabulate (passed in all 15 run 1 runs)
and chunked_even (failed in all 15). With them, no child could win 2 held
out pairs, so every child that passed on train was called overfit.

From the 48 task set, by reading the docstrings and hidden tests (free): run
1's failures were missing argument checks (ValueError for a negative count)
and missing imports. So the picks were tasks whose tests check invalid
arguments or edge cases, plus ncycles, which passed 2 of 3 times in run 1.
Left out: simple recipes with no error tests that Haiku would most likely
always pass (powerset, triplewise, first_true, roundrobin, dotproduct,
iter_index and similar), and tasks that need exact error messages or a
complex API (chunked, reshape). This was a guess without a paid call, and
the run shows it was only half right (see below).

## Results

All children were compared with the root, because none was accepted. Train
is out of 6 tasks per run, held out out of 10.

| # | Version | What it adds | Verdict | Train runs | Train median | Wins | Held out runs | Held out median | Held out wins |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 8e28956a root | | | 4, 3, 3 | 50.0% | | 4, 4, 3 | 40% | |
| 1 | 0ea6c448 control (run 1 gen 1 rule) | raise documented errors, import every module | overfit | 5, 6, 6 | 100% (+50.0) | 3 of 3 | 2, 2, 4 | 20% (-20) | 1 of 3 |
| 2 | 38f0f510 | check the docstring sentence by sentence | overfit | 5, 5, 5 | 83.3% (+33.3) | 3 of 3 | 2, 3, 3 | 30% (-10) | 0 of 3 |
| 3 | 68ae4dea | guard clauses for every argument, imports at the top | noise | 4, 3, 4 | 66.7% (+16.7) | 1 of 3 | 3, 4, 3 | 30% (-10) | 0 of 3 |
| 4 | b2e845f1 | check every name used is imported or defined | noise | 2, 4, 4 | 66.7% (+16.7) | 2 of 3 | 3, 5, 4 | 40% (+0) | 2 of 3 |

The root stays current.

Passes out of 3 per task:

| Task | Split | root | 0ea6c448 | 38f0f510 | 68ae4dea | b2e845f1 |
| --- | --- | --- | --- | --- | --- | --- |
| take | train | 0 | 3 | 3 | 3 | 0 |
| nth | train | 1 | 3 | 3 | 3 | 2 |
| distribute | train | 0 | 3 | 0 | 3 | 0 |
| nth_combination | train | 3 | 3 | 3 | 2 | 2 |
| sort_together | train | 3 | 3 | 3 | 0 | 3 |
| polynomial_from_roots | train | 3 | 2 | 3 | 0 | 3 |
| ncycles | held out | 3 | 1 | 3 | 0 | 3 |
| consume | held out | 3 | 3 | 3 | 3 | 3 |
| windowed | held out | 0 | 0 | 0 | 0 | 0 |
| intersperse | held out | 0 | 0 | 0 | 2 | 0 |
| sliding_window | held out | 2 | 1 | 0 | 3 | 2 |
| locate | held out | 0 | 0 | 0 | 0 | 0 |
| zip_offset | held out | 0 | 0 | 0 | 0 | 1 |
| grouper | held out | 3 | 3 | 2 | 2 | 3 |
| transpose | held out | 0 | 0 | 0 | 0 | 0 |
| circular_shifts | held out | 0 | 0 | 0 | 0 | 0 |

### Does the run 1 rule generalize?

No, not on these tasks. On train it again fixed take, nth and distribute
(train median 50% to 100%, 3 of 3 wins, the same picture as run 1). On the
10 held out tasks it did worse than the root: 20% against 40% median, with
ncycles falling from 3 of 3 to 1 of 3 and sliding_window from 2 to 1. It
helps on the tasks it was written from and does not carry over. The
"overfit" verdict is a real finding this time, not an artifact: held out
wins were possible (gen 4 won 2 of 3 held out pairs).

### The held out set was still too easy to saturate

The free pre-check was only half right. consume passed in all 15 runs, and
windowed, locate, transpose and circular_shifts failed in all 15. So 5 of
the 10 held out tasks gave no signal, and the held out score could move only
on ncycles, intersperse, sliding_window, zip_offset and grouper.

### The 2,048 token cap cost answers

Haiku 5.5 thinks before it answers, and the thinking counts against
`--api-max-new-tokens`. 28 of the 243 calls reached 2,048 output tokens,
and 14 of those were all thinking, with no answer. 19 task runs wrote no
file. windowed accounts for 10 of them, locate 4, zip_offset 2,
nth_combination 2 and grouper 1. windowed failing in all 15 runs is largely
the cap, not the rules. 159,004 of the 253,596 output tokens (63%) were
thinking. For the next run, a larger cap on hard tasks, or turning thinking
off, would buy more answers per dollar than anything else.

### Mutation prompt

A mutation prompt should not suggest things the agent cannot do. Run 1's
rules 2 and 4 asked the agent to run the checks and to read the tests in
`checks/`. The agent answers in one reply and never sees `checks/`, so
those generations were wasted. With `one_shot_agent = true` the prompt says
so, and none of the three new rules asks for either.

## Calls, tokens and cost

From the provider's usage numbers in `api-transcript.jsonl`, at $0.10 per
million input tokens and $0.50 per million output tokens:

| Step | Calls | Input tokens | Output tokens | Cost |
| --- | --- | --- | --- | --- |
| Root, 3 runs | 48 | 26,169 | 42,552 | $0.0239 |
| Control child, 3 runs | 48 | 32,073 | 53,210 | $0.0298 |
| Evolve, 3 generations (3 mutation calls) | 147 | 90,392 | 157,834 | $0.0880 |
| **Total** | **243** | **148,634** | **253,596** | **$0.1417** |

No call failed, timed out or was repeated, so every call is in the log and
the replay. The worst case worked out before the run: 240 task calls at
2,000 input and 2,048 output tokens, plus 3 mutation calls and one redone
run after a possible timeout, came to about $0.32. A guard script would
have stopped the run at $0.40. bogo's own token count is 402,230
(input plus output over all calls).

The free replay (`replay.sh`) reproduces all 23 events of `log.jsonl` after
`init` exactly, apart from the provider name.

## Media

Outside the repo, in `<workdir>/rules-evolve-media/claude-run-2/`: the
`watch` recording sped up 6.5 times (177 s, in color this time), the viewer
lineage and control child pictures, and the final `watch --snapshot`.
