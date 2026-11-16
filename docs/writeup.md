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

## Round 4: one combined run, and nothing was accepted

Finally the 7B ran the whole loop once with both sets in one project: 8 train
and 8 held out tasks, 3 generations, seed 7. 243 calls, 662,708 tokens, 94 min
15 s.

| Version | Change | Train median | Held out median | Verdict |
| --- | --- | --- | --- | --- |
| root | none | 50% (4, 4, 5, 4, 4) | 62.5% (5, 5, 6, 4, 5) | |
| gen 1 | "At the beginning of each Python file, import any necessary modules." | 50% | 62.5% | rejected, 0.0 points, 2 of 5 wins |
| gen 2 | "Import all necessary modules at the beginning of each file." | 50% | 62.5% | rejected, 0.0 points, 0 of 5 wins |
| gen 3 | the same text as gen 2 | | | refused, already in the tree |

This time the model never proposed the narrow `islice` rule. All three ideas
were the generic import rule, which again changed nothing (`take` and `nth`
stayed at 0 of 5). So the same model, seed and tasks took a different path
than the two train only runs, which both found `islice` in generation 2. The
model saw 16 stubs instead of 8 and different failure notes, and llama.cpp
does not give identical answers across runs anyway. No overfit verdict came
up, because nothing passed the train gate.

## Round 5: 48 tasks, and nothing moved

8 tasks per set made every result one or two tasks wide, so the set grew to
48 more-itertools functions: 24 train and 24 held out. The 16 earlier tasks
keep their splits. The 32 new ones come from `recipes.py` and `more.py` and
mix short functions (`ilen`, `mark_ends`), medium ones with edge cases and
keyword arguments (`split_at`, `windowed`, `zip_offset`) and hard ones
(`nth_combination`, `sort_together`, `chunked_even`). Each hidden test was
checked to pass on the upstream code and fail on the stub. Now one task is
about 4.2 points.

Every task prompt shows all 48 stubs, about 9,700 tokens, so the server ran
with a 16,384 token context and each project's token cap per run was raised
from 200,000 to 1,000,000. At about 40 s of generation per call, 5 runs of
every version would have taken about 13 hours. So the root got its 5 runs,
and the oracle, the old `islice` rule and the evolved child got **2 runs
each**, judged on the root's runs 0 and 1 with the same 10 point bars and
every pair required to win (2 of 2). Nov 15 to 16, 10:55 PM to 5:52 AM CT,
529 calls, 5,222,336 tokens:

| Version | Rules | Train runs (of 24) | Train median | Held out runs (of 24) | Held out median | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| root | none | 8, 7, 9, 8, 9 | 33.3% (31.2% on runs 0 and 1) | 6, 5, 8, 6, 5 | 25.0% (22.9%) | |
| control | hand written: import everything you use | 11, 7 | 37.5% | 7, 4 | 22.9% | noise, +6.2 points, 1 of 2 wins; held out +0.0, 1 of 2 |
| old rule | the `islice` rule from round 2 | 8, 5 | 27.1% | 5, 9 | 29.2% | rejected, -4.2 points, 0 of 2 wins; held out +6.2, 1 of 2 |
| evolve gen 1 | "Import common Python functions and utilities at the beginning of each file to avoid frequent import errors." | 9, 6 | 31.2% | 5, 4 | 18.8% | rejected, 0.0 points, 1 of 2 wins; held out -4.2, 0 of 2 |

Nothing generalized, and nothing was accepted. The hand written rule that
gained on both 8 task sets gained 6.2 points on train here and nothing on
held out. The `islice` rule fixed `take` in one of two runs, as before, and
gained on a few held out tasks, but lost `partition` and `exactly_n` and
broke `ncycles` again (2 of 2 to 0 of 2), as it did in round 3. The evolved
rule was once more a generic import rule, and once more it changed nothing.

The root itself moved: on the 8 original train tasks it passed 5 of 8 in 4
of 5 runs, against mostly 4 of 8 when those tasks came with 8 or 16 stubs. More
context changed the answers, so these numbers do not compare one to one with
rounds 2 to 4. 27 of the 48 tasks were never solved by the root in 5 runs,
and 11 always were, so only 10 tasks carry any run to run noise.

## The verdict bar on bigger sets

The bars are in points, so they mean different things at different sizes.
With 8 tasks, 10 points is any one task. With 24 tasks it is 3 tasks (2 tasks
is 8.3 points). That is stricter in tasks but closer to what the rule was
meant to say: a real, repeatable gain.

To see whether the bars still fit, `examples/local-model/verdict-noise.py`
simulated the verdict from the root's measured per task pass rates,
20,000 times per case. A rule either changes nothing, fixes 3 train tasks
the root never solves and nothing else (overfit), or fixes 3 train and 2 or
3 held out tasks (general):

| Rule | Child equals root | Overfit, 3 train tasks | General, 3 + 3 | General, 3 + 2 |
| --- | --- | --- | --- | --- |
| default: 10 points, 4 of 5; held out 10 points, 3 of 5 | 0.0% accepted | 0.3% | 61.5% | 24.8% |
| as run here: 2 runs, 10 points, 2 of 2 on both | 0.0% | 2.0% | 62.8% | 34.5% |
| held out bar 5 points, 3 of 5 | 0.0% | 3.7% | 80.6% | 60.6% |

The train bar is fine: with this little noise a do nothing child is never
accepted. The held out bar is the one that bites. A held out gain of 10
points on 24 tasks needs 3 extra tasks, and a general rule that fixes 3
train tasks usually transfers to fewer, so the default turns most such rules
away. A 5 point held out bar (2 of 24 tasks) with 3 of 5 wins would accept a
3 + 2 rule 61% of the time instead of 25%, at the price of letting a pure
overfit through 3.7% of the time instead of 0.3%. The held out gate's job is
to catch rules that do not transfer, not to demand that they transfer as
strongly as they fit, so a lower held out bar is worth considering for sets
of 20 or more held out tasks. It is a proposal only: the defaults did not
change, and the simulation assumes tasks are independent and that a rule
only lifts tasks the root never solves.

## What this suggests

- A loop that only measures the tasks it tunes on will happily accept a rule
  that fits those tasks. A held out set is cheap and caught it here.
- Specific failure notes make a model's suggestions specific. That is what
  made the 7B useful, and also what made its rule narrow.
- A model has to be able to follow rules before rules can help it. The 3B
  could not, the 7B could.
- The general rule a person wrote from the same failure notes transferred on
  the 8 task sets. On 48 tasks it did not clear the bar (+6.2 train, +0.0
  held out, 2 runs), so even that result was partly small set luck.
- Bigger sets make each verdict cheaper to trust and much more expensive to
  get: 48 tasks with all stubs in the prompt cost about 3 hours per 5 runs on
  this CPU.

## Caveats

- **8 tasks per set** in rounds 1 to 4. One task moves a score by 12.5
  points. The hand written rule's +12.5 on held out tasks is one task's
  worth, just above the bar.
- **Round 5 used 2 runs** for everything but the root, to fit the time. That
  weakens those verdicts: a median of 2 runs is their mean, and 2 of 2 wins is
  easy to miss by one tie.
- **Small models, one machine.** Qwen2.5 Coder 3B and 7B, quantized, on CPU.
  Bigger models may behave differently.
- **Runs are samples.** Even with a fixed seed, llama.cpp did not give
  identical answers across the two 7B runs (root 4, 4, 4, 3, 4, then 4, 4, 5,
  4, 3).
- **The held out verdict was put together by hand** from two projects (the
  train runs and a held out only project), so in the held out runs the model
  saw 8 stub files per task instead of 16. A single combined run with both
  sets in one project was done afterwards (see below), but it did not
  reproduce the `islice` rule, so the overfit verdict was never produced by
  the tool itself in a live run.
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
examples/local-model/replay.sh oss-held-out /path/to/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss-held-out-replay.jsonl
```

Round 5 (about 20 minutes, the checks still run and time out where they did):

```sh
GENERATIONS=1 LATER_REPEATS=2 examples/local-model/replay.sh oss48 /path/to/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss48
```

The more-itertools checkout must be at commit
`ceafee8ce96f4bc1eb74c528bc247c91d648a0bb`. Full details, per task numbers and
failure modes are in `examples/local-model/README.md`.
