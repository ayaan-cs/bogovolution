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
| `replay.sh` | Replays a run you recorded yourself, with no model server |

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
# Apache 2.0 example (1.5B). For a stronger free run, use the 7B Instruct Q4_K_M.
curl -L -o qwen2.5-coder-1.5b-q4km.gguf \
  https://huggingface.co/bartowski/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-Coder-1.5B-Instruct-Q4_K_M.gguf
./llama-b12292/llama-server -m qwen2.5-coder-1.5b-q4km.gguf \
  --host 127.0.0.1 --port 8080 -c 8192 -t 8 --parallel 1 --no-webui
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

## 3. Replay a run you recorded

This repo does not ship recorded answers from the 3B test, because that
model's weights use the Qwen Research license. After you record a run
with an Apache 2.0 model (see `run-local-test.sh`, which writes a replay
file under `$OUT`), replay it for free:

```sh
examples/local-model/replay.sh demo /path/to/demo-replay.jsonl
examples/local-model/replay.sh oss /tmp/more-itertools /path/to/oss-replay.jsonl
```

No model server is needed. The checks still run, so the `oss` replay needs
`python3` and `bwrap`. A replay gives the same pass rates, tokens, hypotheses
and verdicts as the recorded run.

## Results (Nov 11, 2026, 8 vCPU box, no GPU)

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
