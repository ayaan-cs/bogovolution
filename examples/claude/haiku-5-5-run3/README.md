# Claude run 3: claude-haiku-5-5, 28 tasks, pushing the pass rate

Paid run approved by Ayaan on Nov 17, 2026 (11:12 PM CT): up to $2.00 for all
new calls, Haiku first, a cumulative guard that kills paid processes at $1.90
(`guard.sh`, `cost.py`). Real cost of everything in this folder and in
`../sonnet-5-5-run4/`, from the provider's usage fields: **$0.9766**.

| Part | Calls | Input tokens | Output tokens | Cost |
|---|---|---|---|---|
| Calibration A (thinking off, 48 tasks x 2) | 122 | 61,346 | 47,217 | $0.0297 |
| Calibration B (adaptive thinking, 48 tasks x 1) | 48 | 24,285 | 34,503 | $0.0197 |
| First run 3 attempt, thrown away (stale binary, see below) | 46 | 24,148 | 40,709 | $0.0228 |
| Run 3 (this log) | 783 | 610,584 | 973,496 | $0.5478 |
| Run 4, Sonnet (`../sonnet-5-5-run4/`) | 56 | 38,172 | 28,030 | $0.3566 |
| **Total** | 1,055 | | | **$0.9766** |

Prices from the Anthropic pricing page: Haiku 5.5 $0.10 in and $0.50 out per
million tokens, Sonnet 5.5 $2 and $10.

## What changed before the run (free)

- `--api-thinking adaptive|off|between_tools` and `--api-effort`. In run 2,
  28 of 240 replies hit the 2,048 token cap, 14 of them all thinking. This run
  uses adaptive thinking with an 8,192 cap: 0 of 783 replies were cut off.
- Bare path lines: 7 of 383 answers in runs 1 and 2 put `solve/x.py` on its
  own line before the code block instead of `FILE: solve/x.py`. They were
  scored "wrote no files". The parser now accepts that form.
- Transcript lines now carry `stop_reason`, so a cut off reply is visible.

## Diagnosis of run 2 (free, from its transcript)

240 task calls: 112 passed, 100 wrong code, 14 cut off with nothing but
thinking, 14 cut off mid file. `windowed` failed 14 of 15 times on the cap,
`locate` 11 of 15. 5 more answers were lost to the bare path form. The rest
were real code errors, nearly all argument checks the hidden tests expect:
ValueError for a zero or negative size, TypeError for a non iterable,
ValueError for rows of different lengths.

## Task set (chosen from root results only)

Calibration ran the unchanged root rules on all 48 tasks of
`../../local-model/oss48-tasks.sh`, 3 runs per task. Train and held out keep
their 48 task split. Kept: every task the root did not pass 3 of 3 times
(8 train, 13 held out). After the cap fix the always failing ones fail on
real code (unchecked arguments, wrong edge cases), so they stay: a general
rule could fix them. Added as regression guards: 4 always passing train
tasks (flatten, exactly_n, nth_combination, polynomial_from_roots) and 3 held
out ones (ncycles, powerset, mark_ends). No task was edited. Calibration
logs and replays are in `calibration/`.

## Results (repeats 3, seed 7, verdict rule unchanged)

| Version | What | Train runs | Held out runs | Train median | Held out median | Verdict |
|---|---|---|---|---|---|---|
| 8e28956a | root | 8, 10, 8 of 12 | 5, 5, 4 of 16 | 67% | 31% | |
| 0ea6c448 | control: run 1 rule | 11, 11, 9 | 6, 5, 5 | 92% | 31% | overfit |
| b2e845f1 | control: run 2 rule | 10, 8, 10 | 6, 5, 5 | 83% | 31% | noise |
| 1ef4720c | control: hand written rule | 11, 11, 11 | 8, 10, 8 | 92% | 50% | **accepted** |
| 96300d56 | evolve gen | 9, 9, 9 | 6, 6, 4 | 75% | 38% | rejected |
| 442d7084 | evolve gen | 12, 12, 12 | 8, 6, 7 | 100% | 44% | noise |
| 9cbac72d | evolve gen | 11, 11, 10 | 5, 8, 8 | 92% | 50% | rejected |
| 9f88b32d | evolve gen | 12, 11, 12 | 8, 7, 8 | 100% | 50% | noise |
| 93e25c33 | evolve gen | 11, 12, 11 | 7, 7, 6 | 92% | 44% | rejected |

Root to best accepted (1ef4720c): train 67% to 92%, held out 31% to 50%.
Train reached 100% (442d7084, 9f88b32d), but those gained only one train task
(8 points, under the 10 point bar) and did not move held out.

The hand written rule was written by the operator after seeing the root's
train failures in calibration and the run 2 diagnosis (which includes held
out tasks of run 2, some of which are held out here too). It names no task,
function or test; it is general advice (import every name, match the
examples, check arguments like itertools does, check before returning a
generator, raise on mismatched lengths, prefer itertools building blocks).
Treat its held out gain with that in mind. The evolve generations (Haiku as
the mutator, `one_shot_agent = true`) all tried to undo one side effect of it
(`exactly_n` with a negative count started raising ValueError) and none
moved held out.

## What blocks the last gap

The held out failures that remain under every rule need behavior the visible
docstring does not state, for example `chunked_even(it, -1)` must raise
`ValueError("n must be at least 0")` without consuming the input,
`circular_shifts(x, steps=0)` must raise, `locate(..., window_size=0)` must
raise, `before_and_after` must not recurse. A rule can only say "check
arguments" in general; the exact message and the exact cases come from the
hidden tests, and putting them in rules would leak them. Sonnet 5.5 (run 4)
fails the same tasks.

## Notes

- The first run 3 attempt used a release binary built before the bare path
  fix (one answer lost). It was thrown away and the run restarted from a
  fresh project; its cost is counted above.
- The box was restored mid run (about 00:28 CT); every process stopped. The
  hand written control's runs 2 and 3 were redone after a rebuild. The
  replay keeps the later answer for a redone call.
- `replay.sh` replays the whole run for free and must reproduce `log.jsonl`.

Verified on Nov 18, 2026: a free replay into a fresh project gave the same
runs, task results, token counts, hypotheses and verdicts as `log.jsonl`.
