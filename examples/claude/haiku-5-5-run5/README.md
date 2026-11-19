# Claude run 5: claude-haiku-5-5 on the honest example

Paid run approved by Ayaan on Nov 18, 2026. Tasks: `../../honest/` (24 tasks,
16 train and 8 held out), where every behavior a hidden check tests is stated
in the docstring the model fills in. The task set and split were committed
(9bb6104) before any call and not changed afterwards.

Settings as in run 3: adaptive thinking, 8,192 token cap, repeats 3, seed 7,
`one_shot_agent = true`, bwrap isolation, verdict rule unchanged. Guard:
`guard.sh` kills paid processes at $1.90 cumulative for runs 3 and later.

## Cost (from the provider's usage fields)

| Part | Calls | Input tokens | Output tokens | Cost |
|---|---|---|---|---|
| Calibration (root, 2 runs, `calibration/`) | 48 | 23,356 | 38,599 | $0.0216 |
| Run 5 (this log) | 231 | 123,215 | 189,572 | $0.1071 |
| **This run** | 279 | | | **$0.1288** |
| Runs 3 to 5 in all | | | | **$1.1054** |

Prices: Haiku 5.5 $0.10 in and $0.50 out per million tokens. Run 5 has 231
calls instead of 218 because an API timeout stopped one generation part way;
rerunning the same command redid that run.

## Results

| Version | What | Train runs | Held out runs | Train median | Held out median | Verdict |
|---|---|---|---|---|---|---|
| 8e28956a | root | 16, 16, 16 of 16 | 8, 8, 7 of 8 | 100% | 100% | |
| 35ec7146 | evolve gen | 16, 16, 16 | 8, 8, 8 | 100% | 100% | rejected |
| a8bea44a | evolve gen | 16, 16, 16 | 8, 8, 7 | 100% | 100% | rejected |

Calibration (root, 2 more runs): 48 of 48. Over 5 root runs the only miss
was one `human_bytes` answer (held out).

Nothing was accepted, evolved or hand written: with the root at 100% no
child can gain the 10 points the rule needs, so both evolved children were
rejected. No hand written control was run.

## What it shows

On more-itertools (runs 3 and 4) the best Haiku rules reached 92% train and
50% held out, and the rest was behavior the docstrings never state. With
tasks whose docstrings state every checked behavior, the plain starting
rules give 100% on both sets. The gap was the task specs, not the rules.

## Caveats

- These 24 tasks are easier than more-itertools: small, self contained,
  spelled out. 100% here says Haiku follows complete specs, not that it
  solves harder work.
- Calibration found every task always passing. The plan allowed replacing
  degenerate held out tasks at that stage; replacing all 24 with harder ones
  would have meant tuning tasks until the model fails, so the set was kept.
- With nothing left to gain the evolve part demonstrates only that the
  verdict rule does not accept changes without a gain.

`replay.sh` replays the run for free and must reproduce `log.jsonl`.

Verified on Nov 18, 2026: a free replay into a fresh project gave the same
runs, task results, token counts, hypotheses and verdicts as `log.jsonl`.
