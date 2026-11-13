# Can a small model improve its own rules file? Mostly it fit the test.

rules-evolve is a local command line tool that changes a repo's agent rules
file (`AGENTS.md`, `CLAUDE.md` or `.cursor/rules`) one small step at a time and
keeps a step only if it measurably helps. The model stays the same. Only the
rules text changes. Every attempt, good or bad, goes into a hash chained log.
The idea comes from [Petri](https://ethglobal.com/showcase/petri-mjy1y)
(ETHOnline 2026); this is a different, local take on it.

This note tells what happened when real models ran the loop, with the numbers
as they were measured. Everything ran on one 8 vCPU machine without a GPU,
with free models served by llama.cpp on localhost. No paid API was used.

## The setup

- **Tasks.** 8 functions from the Python library more-itertools (`take`,
  `tail`, `nth`, `flatten`, `partition`, `unique_justseen`, `prepend`,
  `quantify`). Each function becomes a stub with its real signature and
  docstring but no body, and the task is to write the body. The library's own
  unit tests for that function, which the model never sees, decide pass or
  fail.
- **Measuring.** Each rules version runs all 8 tasks 5 times. The score of a
  run is the share of tasks passed, and a version's score is the median of its
  5 runs.
- **Keeping a change.** A child version is accepted only if its median beats
  its parent's by at least 10 points and it wins at least 4 of the 5 paired
  runs. If its median is not higher it is rejected. Anything in between is
  noise.
- **The loop.** `evolve` asks the model for one small change plus a one
  sentence hypothesis, benchmarks the child, and moves on from the child only
  if it was accepted. 3 generations, seed 7, each check isolated with bwrap.

With 8 tasks, one task is 12.5 points. Keep that in mind for every number
below.

## Round 1: a 3B model found nothing

Qwen2.5 Coder 3B, no failure details in the prompt. Every version, including
the root, had a median of 37.5%. The model's ideas were generic ("naming
conventions", "variable naming") and all three were rejected (0.0 points
each). The interesting part was the control: a hand written rule that says to
import everything you use, which fixes the most common failure, also scored
37.5% (0.0 points, 3 of 5 wins). The 3B model did not follow even a correct
rule, so no rules file could have helped it on these tasks. Its answers are
not shipped, because that model's license is research only.

## Round 2: a 7B model found a rule that worked

Qwen2.5 Coder 7B Instruct (Apache 2.0), and since v2.2.0 the mutation prompt
includes short notes on why tasks failed. The run committed in the repo took
123 calls, 143,576 tokens and 27 min 12 s:

| Version | Change | Runs (passed of 8) | Median | Verdict |
| --- | --- | --- | --- | --- |
| root | none | 4, 4, 5, 4, 3 | 50% | |
| gen 1 | "Import necessary modules at the beginning of each file." | 4, 4, 5, 4, 3 | 50% | rejected, 0.0 points, 0 of 5 wins |
| gen 2 | "Import the `islice` function from the `itertools` module at the beginning of each relevant file where it is used." | 7, 6, 6, 6, 7 | 75% | accepted, +25 points, 5 of 5 wins |
| gen 3 | the same text as gen 2 | | | refused, identical to its parent |
| control | hand written: import everything you use | 6, 6, 6, 5, 7 | 75% | accepted, +25 points, 5 of 5 wins |

The failure notes worked: every hypothesis named the real failure, `islice`
used without an import. The accepted rule fixed `take` and `nth`, which went
from 0 to 5 of 5 each. A first, independent 7B run (163 calls, 191,899
tokens, 40 min 12 s) accepted the same `islice` rule with the same +25 points
and 5 of 5 wins.

So the loop did its job. But the winning rule names exactly the function
these 8 tasks need. That looks less like a lesson and more like an answer key.

## Round 3: held out tasks showed it overfit

To check, the same rules were run on 8 other more-itertools functions that the
mutating model never saw: `consume`, `roundrobin`, `iter_index`, `tabulate`,
`ncycles`, `powerset`, `unique_everseen` and `first_true`. 5 runs each, seed
7. 160 calls, 311,639 tokens, 86 min 18 s.

| Rules | Held out runs (passed of 8) | Median | vs root | On train |
| --- | --- | --- | --- | --- |
| root | 4, 5, 4, 4, 4 | 50% | | 50% |
| model: generic import rule | 4, 4, 5, 5, 4 | 50% | +0.0, 2 of 5 wins | 50%, rejected |
| model: the `islice` rule | 2, 5, 3, 4, 3 | 37.5% | **-12.5, 0 of 5 wins** | 75%, accepted |
| hand written: import everything you use | 5, 5, 5, 5, 5 | 62.5% | +12.5, 4 of 5 wins | 75%, accepted |

The rule the model got accepted made new tasks worse. It helped `roundrobin` a
little (0 to 2 of 5) but broke `tabulate` (5 to 3 of 5) and `ncycles` (5 to 2
of 5), which the root solved every time. Those were logic errors, not missing
imports: the narrow rule seems to push the model toward `islice` where it does
not belong. The hand written general rule gained on both sets.

Since v2.3.0 the tool supports this directly. Tasks can be marked
`split = "held_out"`; the mutation prompt never shows them; and a child that
wins on the train tasks but does not also gain at least 10 points and 3 of 5
paired wins on the held out tasks gets the verdict **overfit** instead of
accepted. Under that rule the `islice` rule is overfit and the hand written
rule is accepted.

## What this suggests

- A loop that only measures the tasks it tunes on will happily accept a rule
  that fits those tasks. A held out set is cheap and caught it here.
- Specific failure notes make a model's suggestions specific. That is what
  made the 7B useful, and also what made its rule narrow.
- A model has to be able to follow rules before rules can help it. The 3B
  could not, the 7B could.
- The general rule a person wrote from the same failure notes transferred.
  The model did not find it on its own in these runs.

## Caveats

- **8 tasks per set.** One task moves a score by 12.5 points. The hand
  written rule's +12.5 on held out tasks is one task's worth, just above the
  bar.
- **Small models, one machine.** Qwen2.5 Coder 3B and 7B, quantized, on CPU.
  Bigger models may behave differently.
- **Runs are samples.** Even with a fixed seed, llama.cpp did not give
  identical answers across the two 7B runs (root 4, 4, 4, 3, 4, then 4, 4, 5,
  4, 3).
- **The held out verdict was put together by hand** from two projects (the
  train runs and a held out only project), so in the held out runs the model
  saw 8 stub files per task instead of 16. A single combined run with both
  sets in one project was started once and cut short when the machine
  restarted.
- **One library, one kind of task.** All tasks are "write this function" in
  one Python library.

## Reproduce it for free

Every model answer behind these numbers (from the 7B runs) is committed as a
replay file, so the runs can be replayed without a model server, a key or a
network. The checks still run, under bwrap:

```sh
examples/local-model/replay.sh oss /path/to/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss-replay.jsonl \
  examples/local-model/replays/qwen2.5-coder-7b/oss-oracle-replay.jsonl
examples/local-model/replay.sh held-out-eval /path/to/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/held-out-eval-replay.jsonl
```

The more-itertools checkout must be at commit
`ceafee8ce96f4bc1eb74c528bc247c91d648a0bb`. Full details, per task numbers and
failure modes are in `examples/local-model/README.md`.
