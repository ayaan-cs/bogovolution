# Claude run 1: claude-haiku-5-5 on 9 more-itertools tasks

The first run of rules-evolve against a paid model. Ayaan approved it with a
budget well under $1. It ran on the Linux box under bwrap, in a project
outside this repo, on Nov 17, 2026 between 10:04 PM and 10:19 PM CST.

Everything the run produced is here, so it can be shown again for free:

| File | What it is |
| --- | --- |
| `replay.jsonl` | Every model answer the log used (139 calls). `replay.sh` replays it. |
| `replay.sh` | Rebuilds the project from a more-itertools checkout and replays the run. No key, no network. |
| `log.jsonl` | The run's hash-chained log (24 events, `verify-log` OK). |
| `tree.json` | `rules-evolve export` of the log, for the viewer. |
| `api-transcript.jsonl` | Each completed API call: reply, seed, the provider's token usage. The cost below is computed from it. |
| `rules/` | The five rules files (root and four children), named by hash prefix. |
| `config.toml` | The project config. |
| `patch_replay.py` | The one time fix of `replay.jsonl` described under Errors. |

## Setup

- Model: `claude-haiku-5-5`, the cheapest current Haiku on Anthropic's
  pricing page: $0.10 per million input tokens, $0.50 per million output
  tokens. Called with `--provider anthropic --allow-spend --api-model
  claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 2048`.
- Tasks: 9 functions from more-itertools at ceafee8 (MIT), made with
  `make-oss-fixture.py --short-context`. Train: take, nth, distribute,
  nth_combination, sort_together, polynomial_from_roots. Held out: ncycles,
  tabulate, chunked_even. The upstream tests are hidden in `checks/`.
- `repeats = 3`, `min_paired_wins = 3`, `held_out_min_paired_wins = 2`,
  `max_tokens_per_run = 60000`, `isolation = "bwrap"`, seed 7.
- Root rules: "Write clear code and keep changes small."
- Worst case bound: every run stops at the 60,000 token cap, so 12 runs
  (root plus 3 children, 3 runs each) priced entirely as output tokens cost
  about $0.36, plus well under a cent for the mutation calls. With the
  extra generation (see Errors) the bound is 15 runs, $0.45. The real cost
  was about $0.07.

## Results

Every child was compared with the root, because none was accepted.
Pass counts are per run, train out of 6 and held out out of 3.

| Gen | Version | Verdict | Train runs | Train median | Paired wins | Held out runs | Held out median | Held out wins |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 8e28956a (root) | | 4, 2, 2 | 33.3% | | 1, 2, 2 | 66.7% | |
| 1 | 0ea6c448 | overfit | 6, 6, 5 | 100.0% (+66.7) | 3 of 3 | 2, 1, 2 | 66.7% (+0.0) | 1 of 3 |
| 2 | 282aecf2 | noise | 4, 4, 5 | 66.7% (+33.3) | 2 of 3 | 2, 2, 2 | 66.7% (+0.0) | 1 of 3 |
| 3 | ecf0665b | overfit | 5, 5, 4 | 83.3% (+50.0) | 3 of 3 | 2, 2, 2 | 66.7% (+0.0) | 1 of 3 |
| 4 | 1117b7aa | overfit | 5, 5, 5 | 83.3% (+50.0) | 3 of 3 | 2, 2, 2 | 66.7% (+0.0) | 1 of 3 |

The root stays current. What the children added:

1. Re-read the docstring, raise the documented errors, import every module.
2. Run the checks and fix every failure (the model cannot run anything here).
3. Turn every docstring sentence into a checklist item.
4. Read the tests in `checks/` first (they are hidden from the model).

Rules 2 and 4 ask for things the agent cannot do: it answers in one reply,
runs nothing and never sees `checks/`. The mutation prompt should not invite
such rules. Since run 2, `[mutation] one_shot_agent = true` tells the
mutation model so (see `../haiku-5-5-run2/`).

Passes out of 3 per task:

| Task | Split | root | gen 1 | gen 2 | gen 3 | gen 4 |
| --- | --- | --- | --- | --- | --- | --- |
| take | train | 0 | 3 | 3 | 2 | 3 |
| nth | train | 1 | 3 | 3 | 3 | 3 |
| distribute | train | 0 | 3 | 0 | 0 | 0 |
| nth_combination | train | 2 | 3 | 3 | 3 | 3 |
| sort_together | train | 2 | 2 | 1 | 3 | 3 |
| polynomial_from_roots | train | 3 | 3 | 3 | 3 | 3 |
| ncycles | held out | 2 | 2 | 3 | 3 | 3 |
| tabulate | held out | 3 | 3 | 3 | 3 | 3 |
| chunked_even | held out | 0 | 0 | 0 | 0 | 0 |

### How to read the overfit verdicts

The held out set was too small to judge anything. chunked_even failed in
all 15 runs and tabulate passed in all 15, so a held out run could only
score 1 or 2 of 3. The root already scored 2 in two of its three runs, so
no child could win the 2 held out pairs the gate needs. Every child that
passed the train rule was bound to be called overfit. The verdicts are
correct under the configured rule, but they say nothing about whether the
rules generalize. Gen 1 is the interesting one: it fixed take, nth,
distribute and nth_combination on train, and its error and import advice
is general. A bigger held out set is needed before trusting either reading.

## Calls, tokens and cost

From the provider's usage numbers in `api-transcript.jsonl`:

| | Calls | Input tokens | Output tokens | Cost |
| --- | --- | --- | --- | --- |
| Completed calls | 147 | 84,703 | 115,676 | $0.0663 |
| of which used by the log | 139 | | | |
| of which discarded (see Errors) | 8 | 4,251 | 5,657 | $0.0033 |
| Call that timed out | 1 | unknown | unknown | at most about $0.0012 |
| Probe call before the run | 1 | 21 | 5 | under $0.0001 |

Cost = input tokens x $0.10 / 1M + output tokens x $0.50 / 1M. Output
tokens include 63,347 thinking tokens. Total: about $0.067, at most $0.068
if the timed out call was billed in full. One call reached the 2,048 token
cap. rules-evolve's own count (sum of input plus output over the calls the
log used) is 190,471 tokens. A first probe that sent `temperature` got
HTTP 400 and was not billed; that is why `--api-no-temperature` exists.

## Errors

- `claude-haiku-5-5` refuses `temperature`. Added `--api-no-temperature`.
- At 10:13 PM an API call timed out (child 282aecf2, run 0, task
  chunked_even). v2.7.2 stopped without recording the run. The same command
  resumed it at 10:14 PM. That command still said `--generations 3`, so the
  resumed session finished generation 2 and made two more: 4 generations
  in all, one more than planned. The extra generation cost about $0.013.
- Replay bug found by this run: the recorder skipped calls whose key was
  already in the file, so the 8 calls redone after the timeout kept the
  answers of the discarded attempt, and the first free replay disagreed
  with the log (4 overfit instead of 1 noise and 3 overfit). Fixed in
  `replay.rs`: a call made again with a different answer is appended, and
  on load the later entry wins. `patch_replay.py` appended the 8 answers
  the log used, read from the transcript. The replay now reproduces every
  event of `log.jsonl` exactly (only the provider name differs).

## Media

Not in the repo (outside, in `<workdir>/rules-evolve-media/claude-run-1/`):
the `watch` screen recording sped up 5 times (177 s), the viewer lineage
and gen 1 pictures, and the final `watch --snapshot`.
