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
| `replay.sh` | Replays a recorded run, with no model server |
| `replays/qwen2.5-coder-7b/` | The recorded answers of the Qwen2.5 Coder 7B Instruct run below (Apache 2.0 model). `oss-replay.jsonl` is the 3 generation `evolve`, `oss-oracle-replay.jsonl` the hand written control |

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
`WORK`. The run uses `--isolation bwrap`, because what the model writes is
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
  This test has no held out set.
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
