# Local model test (free, localhost only)

This folder runs rules-evolve against a real but small open model that runs on
your own machine. No API key, no account, no spend, and no call leaves the
machine: the `local` provider refuses any URL that is not `localhost`,
`127.0.0.1` or `[::1]`.

## What is here

| File | What it is |
| --- | --- |
| `run-local-test.sh` | Sets up a throwaway project and runs `evolve` with `--provider local`, recording a replay file and a raw transcript |
| `make-oss-fixture.py` | Builds a small task set derived from [more-itertools](https://github.com/more-itertools/more-itertools) (MIT): one stub per function (upstream signature and docstring) and the upstream unit tests as hidden checks. It is derived from a real project, not the whole project. The more-itertools code is not copied into this repo |
| `oracle-rules.md` | A hand written rules change used as a control (see results) |
| `oss48-tasks.sh` | The 48 task set: 24 train and 24 held out functions |
| `validate-oss-fixture.sh` | Checks every task `make-oss-fixture.py` knows: the hidden check passes on the upstream code and fails on the stub |
| `run-oss48.sh` | The 48 task run in restartable stages: root, control, the old `islice` rule, then a combined `evolve` |
| `verdict-noise.py` | Simulates how often the verdict rule accepts, from a root's measured runs |
| `replay.sh` | Replays a recorded run, with no model server |
| `replays/qwen2.5-coder-7b/` | The recorded answers of the Qwen2.5 Coder 7B Instruct runs below (Apache 2.0 model). `oss-replay.jsonl` is the 3 generation `evolve`, `oss-oracle-replay.jsonl` the hand written control, `held-out-eval-replay.jsonl` the held out check (v2.3.0), with the two model written rules it used as `rules-*.md`, `oss-held-out-replay.jsonl` the combined run of v2.6.0, `oss48/` the 48 task run (root, control, `islice` rule, evolve, and the rule the evolve wrote), and `oss48-short/` the 5 run rerun with short prompts (root, control, evolve) |

## 1. Get a model server (free)

Tested with the prebuilt CPU build of llama.cpp. No account or token is
needed for the download. Prefer an Apache 2.0 model so the answers you
record can sit in this repo. Qwen2.5 Coder 1.5B Instruct and 7B Instruct
are Apache 2.0. The 3B Instruct weights use the Qwen Research license,
which is for non-commercial research only, so this repo does not ship
any of that model's output.

```sh
mkdir -p ~/llm && cd ~/llm
curl -LO https://github.com/ggml-org/llama.cpp/releases/download/b12292/llama-b12292-bin-ubuntu-x64.tar.gz
tar xzf llama-b12292-bin-ubuntu-x64.tar.gz
# Apache 2.0, the model used for the results below (4.7 GB, about 5 GB of RAM).
# License: https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/blob/main/LICENSE
curl -L -o qwen2.5-coder-7b-instruct-q4_k_m.gguf \
  https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/main/qwen2.5-coder-7b-instruct-q4_k_m.gguf
# sha256 509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c
./llama-b12292/llama-server -m qwen2.5-coder-7b-instruct-q4_k_m.gguf \
  --host 127.0.0.1 --port 8080 -c 8192 -t 8 --parallel 1 --no-webui
# smaller and faster, also Apache 2.0: bartowski/Qwen2.5-Coder-1.5B-Instruct-GGUF
# For the 48 task set with every stub in the prompt (about 9,700 tokens) use
# -c 16384. With SHORT_CONTEXT=1 (about 600 tokens) -c 8192 is enough.
# Long runs: add --cache-ram 1024. The default prompt cache (8 GiB) grew the
# server to about 12 GB and got it killed for memory on a 16 GB machine.
```

Any OpenAI compatible server that serves `/v1/models` and
`/v1/chat/completions` on loopback works (llama.cpp, Ollama, LM Studio, vLLM).
Check the model license before you commit or share its output.

## 2. Run the test

```sh
cargo build --release
# the demo fixture (12 toy tasks)
OUT=/tmp/lm-out examples/local-model/run-local-test.sh demo
# 8 tasks derived from more-itertools (pinned commit used here: ceafee8ce96f)
git clone https://github.com/more-itertools/more-itertools /tmp/more-itertools
git -C /tmp/more-itertools checkout ceafee8ce96f4bc1eb74c528bc247c91d648a0bb
OUT=/tmp/lm-out examples/local-model/run-local-test.sh oss /tmp/more-itertools
```

Settings: `GENERATIONS` (default 3), `SEED` (default 7), `LOCAL_URL`, `OUT`,
`WORK`, `OSS_FUNCS` (train functions) and `OSS_HELD_OUT` (held out functions,
since v2.3.0; set it to an empty string for the train only set of v2.2.0).
The default oss set is 8 train and 8 held out functions, so a run takes
about twice as long as the v2.2.0 numbers below. The run uses `--isolation bwrap`, because what the model writes is
executed by the checks. Keep `--record` and `--local-transcript` files outside
the project (or under `.rules-evolve/`): the CLI refuses paths the model would
read as project files.

## 3. Replay for free

The 7B run below is in this repo. Replay it, with the control, in about 15
seconds and with no model server:

```sh
examples/local-model/replay.sh oss /tmp/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss-replay.jsonl \
  examples/local-model/replays/qwen2.5-coder-7b/oss-oracle-replay.jsonl
```

A run you record yourself replays the same way (`run-local-test.sh` writes
the replay file under `$OUT`). This repo does not ship answers from the 3B
test, because that model's weights use the Qwen Research license.

No model server is needed. The checks still run, so the `oss` replay needs
`python3` and `bwrap`. A replay gives the same pass rates, tokens, hypotheses
and verdicts as the recorded run.

## 48 tasks: 24 train, 24 held out

Nov 15 to 16, 2026, Qwen2.5 Coder 7B Instruct Q4_K_M, llama.cpp b12292 with
`-c 16384 -t 8 --parallel 1`, seed 7, bwrap. With 8 tasks per set one task is
12.5 points; with 24 it is about 4.2.

**The set.** The 16 tasks above keep their splits. 32 more come from
`recipes.py` (`dotproduct`, `iter_except`, `polynomial_derivative`,
`grouper`, `subslices`, `sliding_window`, `nth_combination`,
`polynomial_from_roots` for train; `repeatfunc`, `triplewise`,
`before_and_after`, `transpose`, `reshape`, `totient` held out) and from
`more.py` (`ilen`, `duplicates_justseen`, `exactly_n`, `split_at`,
`distribute`, `consecutive_groups`, `stagger`, `sort_together` for train;
`mark_ends`, `longest_common_prefix`, `interleave_longest`,
`circular_shifts`, `intersperse`, `windowed`, `chunked`, `locate`,
`zip_offset`, `chunked_even` held out). Per split: 6 short, 7 medium and 3
hard new tasks. `make-oss-fixture.py` now reads both upstream files; stubs
are still signature and docstring only, and `--reference` (local validation
only) fills them with the whole upstream module. `validate-oss-fixture.sh`
accepts a task only if its hidden check passes on upstream code, fails on
the stub, and uses no other more-itertools name. 59 tasks pass; 10
candidates were dropped: their tests call other library functions
(`mi.take`, `mi.ilen`, ...), a test only helper or the package itself, or
did not pass on the upstream code in this layout.

**What it costs.** Every task prompt shows all 48 stubs (about 9,700
tokens), so the server needs `-c 16384`, and the project's
`max_tokens_per_run` must be raised from 200,000 (one run uses about 475,000;
the first attempt stopped after 20 tasks per run because of it). Generation
ran at about 6 tokens per second, about 40 s per call, so 5 runs of one
version (240 calls) took 3 h 17 min.

**What was cut.** Root, control, `islice` rule and a 1 generation `evolve` at
5 runs each would have been about 960 calls, around 13 hours, against a
budget of about 6. The root got its 5 runs; the later stages ran with
`LATER_REPEATS=2`: 2 runs each, judged on the root's runs 0 and 1 with the
same 10 point bars and 2 of 2 paired wins on train and held out. That makes
these verdicts weaker than the 5 run ones above.

| Version | Train runs (of 24) | Train median | Held out runs (of 24) | Held out median | Verdict | Calls, tokens, time |
| --- | --- | --- | --- | --- | --- | --- |
| root | 8, 7, 9, 8, 9 | 33.3% (31.2% on runs 0, 1) | 6, 5, 8, 6, 5 | 25.0% (22.9%) | | 240, 2,368,963, 3 h 17 min |
| control (`oracle-rules.md`) | 11, 7 | 37.5% | 7, 4 | 22.9% | noise: +6.2, 1 of 2 wins; held out +0.0, 1 of 2 | 96, 951,929, 1 h 12 min |
| `islice` rule (round 2) | 8, 5 | 27.1% | 5, 9 | 29.2% | rejected: -4.2, 0 of 2; held out +6.2, 1 of 2 | 96, 950,122, 1 h 13 min |
| evolve gen 1 | 9, 6 | 31.2% | 5, 4 | 18.8% | rejected: 0.0, 1 of 2; held out -4.2, 0 of 2 | 97, 951,322, 1 h 15 min |

The evolve ran from a copy of the project that already held the root runs,
so it did not run the root again. Its one change: "Import common Python
functions and utilities at the beginning of each file to avoid frequent
import errors." (`oss48/rules-gen1-8456ab49.md`). In all, 529 calls and
5,222,336 tokens, Nov 15 10:55 PM to Nov 16 5:52 AM CT.

Nothing was accepted and no rule generalized. Per task, passes in runs 0
and 1 against the root's: the control gained `take` (0 to 1), `iter_except`
(0 to 2), `duplicates_justseen`, `iter_index` and `interleave_longest` (0 to
1 each) and lost one pass each on `exactly_n`, `first_true` and
`repeatfunc`. The `islice` rule gained `take`, `unique_justseen`,
`iter_except`, `roundrobin` (0 to 1 each), `iter_index` and
`interleave_longest` (0 to 2), but lost `tail` (1 to 0), `partition` and
`exactly_n` (2 to 0 each), and again broke `ncycles` (2 to 0), as in the
v2.3.0 check. 27 of the 48 tasks were never
solved by the root and 11 always were; only 10 carry run to run noise.

**The verdict bar.** `verdict-noise.py` simulates the rule from the root's
per task pass rates (20,000 pairs per case). With this root, a child that
changes nothing is never accepted, under the default or the 2 run setup. A
rule that fixes 3 train tasks and nothing held out passes 0.3% of the time
(default) and 2.0% (2 runs). A general rule that fixes 3 train and 3 held out
tasks passes about 62% of the time either way, and one that fixes 3 train
and 2 held out tasks only 25% (default). A 5 point held out bar with 3 of 5
wins would lift that to 61% and let a pure overfit through 3.7% of the time.
`docs/writeup.md` has the reasoning. The defaults did not change.

Replay it (about 20 minutes, the checks still run; it reproduced every number above):

```sh
GENERATIONS=1 LATER_REPEATS=2 examples/local-model/replay.sh oss48 /tmp/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss48
```

Run it yourself, restartable after a crash or reboot (finished stages are
skipped; runs already in the log are not run again):

```sh
OUT=~/oss48-out WORK=~/oss48-work LATER_REPEATS=2 \
  examples/local-model/run-oss48.sh /tmp/more-itertools
```

### 48 tasks again: short prompts, 5 runs each (v2.7.0)

Nov 16, 2026, same model and seed, llama.cpp b12292 with `-c 8192 -t 8
--parallel 1`, bwrap, `SHORT_CONTEXT=1`: each task's `context` is its own
stub, so a task prompt is about 600 tokens instead of about 9,700.

**Speed.** 5 runs of the root took 1 h 47 min and 143,564 tokens, against
3 h 17 min and 2,368,963 tokens with every stub shown: about 27 s per call
instead of about 49 s, and 16 times fewer tokens. Generation also ran faster
(about 10.5 against 6 tokens per second).

**Plan.** Root, control and a 3 generation `evolve` at the default 5 runs,
with the standard bars: train 10 points and 4 of 5 wins, held out (24 tasks,
so the v2.7.0 large set bar) 5 points and 3 of 5 wins. The `islice` rule was
put off (marker `islice.done`) to keep the run under about 10 hours.

| Version | Train runs (of 24) | Train median | Held out runs (of 24) | Held out median | Verdict | Calls, tokens, time |
| --- | --- | --- | --- | --- | --- | --- |
| root | 9, 10, 9, 9, 9 | 37.5% | 8, 6, 5, 5, 7 | 25.0% | | 240, 143,564, 1 h 47 min |
| control (`oracle-rules.md`) | 9, 9, 12, 11, 10 | 41.7% | 6, 4, 5, 6, 4 | 20.8% | noise: +4.2, 3 of 5 wins; held out -4.2, 1 of 5 | 240, 156,108, 1 h 48 min |
| evolve gen 1 | 9, 10, 8, 9, 8 | 37.5% | 6, 5, 6, 6, 5 | 25.0% | rejected: 0.0, 0 of 5; held out 0.0, 2 of 5 | 241, 147,836, 1 h 49 min |
| evolve gen 2 | 10, 9, 8, 9, 9 | 37.5% | 6, 6, 6, 6, 6 | 25.0% | rejected: 0.0, 1 of 5; held out 0.0, 2 of 5 | 241, 151,265, 1 h 50 min |
| evolve gen 3 (first try) | 8, 0, 0, 0, 0 | | 0, 0, 0, 0, 0 | | logged rejected (-37.5), but invalid and not counted: the server was killed | 22, 13,017, 9 min |
| evolve gen 3 (rerun) | 9, 7, 7, 8, 10 | 33.3% | 7, 5, 8, 6, 4 | 25.0% | rejected: -4.2, 1 of 5; held out 0.0, 2 of 5 | 241, 147,647, 1 h 57 min |

The evolve's three changes were all generic import rules:
"Always import necessary Python libraries at the beginning of the file.",
"Import common Python libraries at the beginning of each file where they
are needed. This will prevent errors related to unimported functions or
classes." and "Import common Python libraries at the beginning of each
file." (`oss48-short/rules-gen*.md`). In all 984 calls and 611,790 tokens,
Nov 16 8:04 AM to 3:27 PM CT, then 241 calls and 147,647 tokens for the
generation 3 rerun, 4:18 to 6:15 PM CT: 1,225 calls, 759,437 tokens and
9 h 20 min of model time, of which 1,203 calls and 746,420 tokens count.

Per task, passes of 5 against the root's: the control fixed `take` (0 to 4)
and `nth` (0 to 3), the `islice` tasks it was written for, and lost
`exactly_n` (4 to 2), `triplewise` (3 to 0), `mark_ends` (3 to 1) and
`interleave_longest` (3 to 1). Gen 1 and 2 did not fix `take` or `nth`; both
lost `unique_justseen` (5 to 2) and gained `nth_combination` (0 to 2, 3) and
`repeatfunc` (1 to 3, 4). Nothing was accepted and nothing generalized.

**Generation 3 is not a result.** llama.cpp's prompt cache (`--cache-ram`,
8 GiB by default) grew the server to about 12 GB, and at 3:27 PM CT the
kernel killed it, 22 calls into generation 3. rules-evolve logged every
later call as a model error, so 27 of 48 tasks in run 0 and all of runs 1 to
4 failed, and the generation was logged as rejected at -37.5 points.
It was rerun alone: a copy of the project replayed generations 1 and 2
from `evolve-replay.jsonl`, then `evolve --generations 1` ran live with
`--cache-ram 1024` (server steady at about 6.2 GB) and recorded
`evolve-gen3-replay.jsonl`. Same mutation seed, same prompt, same rule; it
lost `tail` (5 to 3), `unique_justseen` (5 to 2), `mark_ends` (3 to 0) and
`interleave_longest` (3 to 1) and gained a few single passes.

**The verdict bar, on this root.** `verdict-noise.py`, 20,000 pairs per
case: a child that changes nothing is accepted 0.0% of the time under both
held out bars. A pure overfit (3 train tasks, nothing held out) passes 0.3%
with a 10 point held out bar and 3.8% with 5. A general rule that fixes 3
train and 3 held out tasks passes 55.3% and 73.8%; one that fixes 3 and 2,
22.8% and 55.1%.

Replay it (about 1 hour 15 minutes, the checks still run; it reproduced
every counted number above). The first try's generation 3 cannot be
replayed, because the calls it never made are not in the file
(`GENERATIONS=3` stops there with a replay miss); `NEXT_REPLAY` replays the
rerun instead.

```sh
SHORT_CONTEXT=1 GENERATIONS=2 NEXT_REPLAY=evolve-gen3-replay.jsonl \
  examples/local-model/replay.sh oss48 /tmp/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss48-short
```

Run it yourself (restartable; add `--cache-ram 1024` to the server):

```sh
OUT=~/oss48-out WORK=~/oss48-work SHORT_CONTEXT=1 GENERATIONS=3 \
  examples/local-model/run-oss48.sh /tmp/more-itertools
```

## Held out check of the 7B result (v2.3.0)

Nov 11, 2026, same machine, model, server and settings as the v2.2.0 run
below. Question: does the rule the 7B model got accepted (`islice`) help on
tasks it was not tuned on? The 8 held out functions are related to the train
ones but not the same: 3 use `islice` like `take` and `nth` (`consume`,
`roundrobin`, `iter_index`), 4 need other `itertools` or `collections`
imports (`tabulate`, `ncycles`, `powerset`, `unique_everseen`), and 1 needs
no import (`first_true`). Each was checked with `--reference` (the upstream
code passes its tests) and without it (the stub fails).

To keep it cheap, only the held out tasks were run: a project with just those
8 tasks, the root plus the three earlier rule texts proposed from the root,
5 runs each with `run --seed 7`. The train numbers are the recorded v2.2.0
ones. 160 calls, 311639 tokens, 86 min 18 s (about 32 s per call; held out
answers are longer, and the box was slower than in the earlier run).

| Rules | Held out runs (passed of 8) | Median | vs root | Train (v2.2.0) | v2.3.0 verdict |
| --- | --- | --- | --- | --- | --- |
| root | 4, 5, 4, 4, 4 | 50% | | 50% | |
| gen 1 "Import necessary modules at the beginning of each file." | 4, 4, 5, 5, 4 | 50% | +0.0 points, 2 of 5 wins | 50%, 0 of 5 | rejected (by train) |
| gen 2 "Import the `islice` function ... where it is used." | 2, 5, 3, 4, 3 | 37.5% | **-12.5 points, 0 of 5 wins** | 75%, +25, 5 of 5 | **overfit** |
| control `oracle-rules.md` | 5, 5, 5, 5, 5 | 62.5% | +12.5 points, 4 of 5 wins | 75%, +25, 5 of 5 | accepted |

Per task, passes of 5 (root / generic / islice / control): `consume` 0/0/0/0,
`roundrobin` 0/0/2/1, `iter_index` 0/0/0/2, `tabulate` 5/5/3/5, `ncycles`
5/5/2/5, `powerset` 4/5/5/5, `unique_everseen` 2/3/1/2, `first_true` 5/4/4/5.

What this shows:

- The accepted `islice` rule does not generalize. It helped `roundrobin` a
  little (0 to 2 of 5) but broke `tabulate` and `ncycles`, which the root
  solved every time. Those failures were logic errors (for example
  `range(start, None)` in `tabulate`, the wrong repeat count in `ncycles`),
  not import errors: the narrow rule seems to push the model toward
  `islice` where it does not fit. Under the v2.3.0 gate it is overfit and
  `current` would stay at the root.
- The hand written general rule passes the gate (+12.5, 4 of 5), mostly via
  `iter_index` (0 to 2) and `roundrobin` (0 to 1) and no losses.
- The generic model rule is flat on both sets, as on train.
- Limits: the verdicts in the last column are put together by hand from two
  projects, not by one `verify`, and in the held out only project the model
  saw 8 stub files per task, not the 16 of a combined project. With 8 tasks a
  single function moves the rate by 12.5 points, so the control's +12.5 is
  one task's worth and just above the bar. `consume` failed for every rule
  (its tests saw the wrong number of items left, for example `3 != 0`).

**The combined 7B `evolve` (8 train plus 8 held out tasks in one project).**
A first attempt on Nov 11 was cut short when the box restarted. It was run
again on Nov 13, 2026 from 1:52 AM to 3:26 AM (America/Chicago) with the
defaults: 3 generations, seed 7, bwrap, the same model and server settings.
243 calls, 662708 tokens, 5655 s (94 min 15 s). The replay is
`replays/qwen2.5-coder-7b/oss-held-out-replay.jsonl`, and replaying it
reproduces every run, verdict and hash of the rules versions (117 s, no model
server).

| Version | Rules change | Train runs (of 8) | Train median | Held out runs (of 8) | Held out median | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| root | none | 4, 4, 5, 4, 4 | 50% | 5, 5, 6, 4, 5 | 62.5% | |
| gen 1 | "At the beginning of each Python file, import any necessary modules." | 4, 5, 4, 4, 5 | 50% | 3, 5, 4, 5, 5 | 62.5% | rejected, 0.0 points, 2 of 5 wins (held out 0.0, 1 of 5) |
| gen 2 | "Import all necessary modules at the beginning of each file." | 4, 4, 4, 4, 4 | 50% | 4, 5, 5, 5, 5 | 62.5% | rejected, 0.0 points, 0 of 5 wins (held out 0.0, 1 of 5) |
| gen 3 | the same text as gen 2 | | | | | refused: already in the tree |

Nothing was accepted and nothing was overfit. This time the model never
proposed the narrow `islice` rule: all three hypotheses were the same generic
"import necessary modules" idea, which, as in the earlier runs, did not change
what the model wrote. Per task, passes of 5 (root / gen 1 / gen 2): train
`take` 0/0/0, `tail` 1/1/1, `nth` 0/0/0, `flatten` 5/5/5, `partition` 4/5/4,
`unique_justseen` 1/1/0, `prepend` 5/5/5, `quantify` 5/5/5; held out
`consume` 0/0/0, `roundrobin` 1/0/0, `iter_index` 0/0/0, `tabulate` 5/5/5,
`ncycles` 5/5/5, `powerset` 5/5/5, `unique_everseen` 4/3/5, `first_true`
5/4/4. So the same model, seed and tasks gave a different mutation path than
the two train only runs (which both found `islice` in generation 2): with
held out tasks in the project the model saw 16 stubs and different failure
notes, and llama.cpp is not deterministic across runs anyway. One run is a
sample, not a law.

Replay it for free:

```sh
examples/local-model/replay.sh oss-held-out /tmp/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/oss-held-out-replay.jsonl
```

Replay the held out check (about 4 minutes, no model server):

```sh
examples/local-model/replay.sh held-out-eval /tmp/more-itertools \
  examples/local-model/replays/qwen2.5-coder-7b/held-out-eval-replay.jsonl
```

## Results with Qwen2.5 Coder 7B Instruct and failure notes (v2.2.0)

Nov 11, 2026, 8 vCPU box, no GPU. Model: Qwen2.5 Coder 7B Instruct, Q4_K_M
GGUF from the official Qwen repo (Apache 2.0), llama.cpp b12292, context 8192,
temperature 0.7, at most 1024 new tokens per call, seeds passed through.
Default rule: 5 repeats, accept at a median gain of 10 points and 4 of 5
paired wins. Same 8 more-itertools tasks as the 3B run, 3 generations, seed 7,
`--isolation bwrap`. Generation speed: median 10.4 tokens per second (1.8 to
11.3). The mutation prompt now carries the parent's benchmark results and
failure notes (v2.2.0). The demo was not run again: its check prints nothing,
so failure notes cannot tell a model the hidden answer path, and the 3B result
there (0% everywhere) says nothing new.

**The run in `replays/qwen2.5-coder-7b/`.** 123 calls, 143576 tokens,
27 min 12 s.

| Version | Rules change | Runs (passed of 8) | Median | Verdict |
| --- | --- | --- | --- | --- |
| root | none | 4, 4, 5, 4, 3 | 50% | |
| gen 1 | "Import necessary modules at the beginning of each file." | 4, 4, 5, 4, 3 | 50% | rejected, 0.0 points, 0 of 5 wins |
| gen 2 | "Import the `islice` function from the `itertools` module at the beginning of each relevant file where it is used." | 7, 6, 6, 6, 7 | 75% | **accepted, +25.0 points, 5 of 5 wins** |
| gen 3 | the same text as gen 2 | | | refused: identical to the parent |
| control | hand written `oracle-rules.md` (import everything you use; its examples name `islice` too) | 6, 6, 6, 5, 7 | 75% | accepted, +25.0 points, 5 of 5 wins (40 calls, 48263 tokens, 9 min 10 s) |

The control was recorded against the first 7B run below; its task calls do
not depend on the mutation prompt, so it replays against this lineage too
(paired with this root: 4, 4, 5, 4, 3).

Per task, root to gen 2: `take` 0 to 5 of 5 and `nth` 0 to 5 of 5 (both had
failed with `NameError: name 'islice' is not defined`); `unique_justseen` 1 to
3 of 5; `flatten`, `prepend`, `quantify` 5 of 5 both; `partition` 4 to 4;
`tail` 0 to 0 (a real logic error: `islice` with a negative index).

**A first 7B run** (same settings, before a redaction fix, see below; kept
outside the repo) gave the same picture: root 4, 4, 4, 3, 4 (50%); gen 1 the
same generic import rule, rejected (0.0 points, 1 of 5 wins); gen 2 the same
`islice` rule, accepted (6, 6, 7, 6, 7, +25.0 points, 5 of 5 wins); gen 3 added
`groupby` and `filterfalse` imports to the accepted rule and was rejected
(4, 5, 4, 4, 5, -25.0 points, 0 of 5 wins: `nth` went back to failing). 163
calls, 191899 tokens, 40 min 12 s. Even with a fixed seed, llama.cpp did not
give identical answers across the two runs (root 4, 4, 4, 3, 4 then 4, 4, 5,
4, 3), so treat single runs as samples.

What this shows, and what it does not:

- With failure notes, the mutations became specific. Every hypothesis named
  the real failure (`islice` not defined). Without notes, the 3B model only
  produced "naming conventions" guesses.
- The 7B model follows rules the 3B did not: the same hand written import
  rule that changed nothing for the 3B gave +25 points for the 7B.
- Generic wording did not move the 7B ("Import necessary modules": no change
  at all), but a concrete rule naming the function did.
- The accepted rule is narrow. It names `islice`, which is exactly what these
  8 tasks need, so it is close to fitting the benchmark. On a real repo, a
  rule this specific would need a held out task set to show it generalizes.
  This test had no held out set; the v2.3.0 check above shows it does not
  generalize.
- The verdict rule behaved well: equal medians were rejected, a 25 point drop
  was rejected, and both accepts won all 5 pairs.
- Failure modes seen: repeated hypotheses (gen 1 and gen 3 near identical),
  a mutation identical to its parent (refused, as designed), and a rule that
  hurt a task it was not about (`nth` in the first run's gen 3). No timeouts,
  model errors, budget hits or format refusals.
- Redaction fix found during the first run: unittest prints the first line of
  a test docstring, without its quotes, so 3 docstring lines of the hidden
  tests reached the notes. Output lines that are part of a hidden line are now
  redacted too, and the second run (the one in this repo) has none.

## Earlier result: Qwen2.5 Coder 3B, no failure notes (v2.1.0)

The numbers below are from a private Nov 11, 2026 run with Qwen2.5 Coder
3B Instruct (Q4_K_M, llama.cpp b12292, context 8192, temperature 0.7, at
most 1024 new tokens per call, seeds passed through). The model's answers
are not in this repo. The figures are measurements of that run, not its
output.
Rule: the default (5 repeats, accept at a median gain of 10 points and 4 of 5
paired wins). Generation speed was about 20 tokens per second when the box was
quiet and dropped to 1 to 15 when other work shared the CPU.

**Demo fixture, 3 generations.** 243 calls, 52825 tokens, 3 min 57 s. Every
run of every version passed 0 of 12. All 3 mutations were rejected (0.0 points,
0 of 5 wins). This is expected: the demo checks look for `answers/<id>.txt`,
which nothing in the prompt or the visible files mentions, so only a rule that
names the path could help, and the mutation prompt carries no failure details
from which a model could guess it.

**more-itertools tasks, 3 generations.** 8 functions (`take`, `tail`, `nth`,
`flatten`, `partition`, `unique_justseen`, `prepend`, `quantify`), picked
from a 17 function baseline (13 of 34 attempts passed there). 163 calls,
189918 tokens, 31 min 42 s.

| Version | Runs (passed of 8) | Median | Verdict |
| --- | --- | --- | --- |
| root | 3, 1, 3, 3, 0 | 37.5% | |
| gen 1, naming conventions | 3, 0, 3, 3, 3 | 37.5% | rejected, 0.0 points, 1 of 5 wins |
| gen 2, variable naming | 4, 0, 4, 3, 2 | 37.5% | rejected, 0.0 points, 3 of 5 wins |
| gen 3, naming conventions again | 3, 0, 3, 4, 1 | 37.5% | rejected, 0.0 points, 2 of 5 wins |
| control, hand written import rule | 4, 0, 4, 3, 2 | 37.5% | rejected, 0.0 points, 3 of 5 wins (40 calls, 48123 tokens, 4 min 24 s) |

Nothing was accepted. What the transcripts show:

- The model's three mutations were all generic ("naming conventions") and
  near duplicates of each other. The mutation prompt has only the rules and
  the lineage history, no failing task output, so a small model guesses.
- The dominant task failure is real: the model uses `islice`, `chain` and
  friends without importing them. A hand written rule that says exactly that
  (`oracle-rules.md`) did not change the behavior: the 3B model kept leaving
  the imports out. So at this size the rules text barely moves the outcome.
- Outcomes depend far more on the seed than on the rules: run index 1 scored
  0 or 1 of 8 for every version, mostly because with that seed the model wrote
  a bare code block without the `FILE:` line, which the harness reads as no
  edit. Pairing by seed is doing its job here.
- No refusals, no timeouts, no model errors, no budget hits. One malformed
  reply put `FILE:` inside a fence, which became a syntax error and a failure.
- The verdict rule behaved sensibly: per run swings of 0 to 50 points did not
  produce a false accept, and equal medians were rejected.

**Conclusion.** The pipeline works end to end with a real local model
(isolated checks, records, replays, digest, export). This run cannot say
whether rules-evolve improves a rules file: a 3B model does not follow even a
correct rule reliably, so the measurement is dominated by sampling noise.
A stronger model, more generations, or failure details in the mutation prompt
would be needed to get a real signal.
