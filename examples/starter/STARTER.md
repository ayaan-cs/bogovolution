# Starter: use bogo on your own repo

bogo changes your agent rules file (AGENTS.md, CLAUDE.md,
.cursor/rules) one small step at a time and keeps a change only when a
benchmark of tasks from your repo says it helps. This folder is a template
for that benchmark:

| File | What it is |
| --- | --- |
| `tasks.toml` | A heavily commented task file to copy and edit. |
| `sample-repo/` | A tiny Python package (`textkit`) with 7 bugs and hidden tests in `checks/`. |
| `mock.toml` | A free stand in model for the sample, so the whole loop runs without a key. |
| `try.sh` | Runs the sample end to end, free: self check, mock evolve, replay. |
| `selfcheck.py` | Proves every task's check passes with the right fix and fails without it. |

## 1. Try it first (free, under a minute)

```sh
cargo build --release            # once, from the repo root
BOGO=target/release/bogo examples/starter/try.sh
```

It checks the sample tasks, runs `evolve --generations 3 --total` with the
mock model (the checks are the real Python tests, run isolated), shows that
running it again does nothing because the total is reached, then replays the
recording in a fresh copy and confirms the log is identical. The mock
accepts the general rule ("check every case the docstring names") and
rejects the other two. Then look around:

```sh
bogo -C <folder>/mock digest     # what worked, what failed
bogo -C <folder>/mock apply      # diff of the best rules (dry run)
```

## 2. Set up your repo

```sh
cd your-repo
bogo init --model <model id>     # writes .bogo/
cp path/to/examples/starter/tasks.toml .bogo/tasks.toml
```

Then edit `.bogo/config.toml`. For a first run:

```toml
[evaluation]
repeats = 3                    # runs per version (5 is steadier, 3 is cheaper)
min_paired_wins = 3            # with 3 repeats: win every paired run
held_out_min_paired_wins = 2

[mutation]
one_shot_agent = true          # the agent answers in one reply and runs nothing
```

`one_shot_agent = true` tells the model that proposes rule changes that the
agent cannot run commands or read the hidden tests. Without it, half the
proposals in our first Claude run asked for exactly that and were wasted.

## 3. Write the first 5 to 10 tasks from tests you already have

A task is a prompt, a check command, and the files the model may see. The
easiest source is an existing test:

1. Pick a function or module with a focused test file.
2. Break it on purpose in the benchmark copy: replace the body with a stub
   or put back an old bug. The test must now fail.
3. Move the test into a hidden folder (here `checks/`) and list it under
   `[sandbox] hidden`. The model never sees hidden files; they come back
   only when the check runs.
4. Write the prompt a teammate would write: what is wrong, one example,
   which file. Do not describe the test.
5. `check` runs just that test, for example
   `python3 -m unittest -q checks.test_slugify`, `npx vitest run
   checks/slug.test.ts` or `cargo test --test slug`. Exit code 0 is a pass.
6. `context` lists the files the prompt is about. A short list keeps every
   call small and cheap.

All tasks share one copy of the repo, so keep them independent: one broken
function per task, one test file per task, and no two tasks in the same file
if you can help it. Check every task both ways, as `selfcheck.py` does for
the sample: with the right fix the check must pass, without it fail.

## 4. Mine your history for more tasks

Bug fixes and features that came with tests are ready made tasks:

1. Find commits or merged pull requests that changed code and added or
   changed a test (`git log --stat -- tests/`).
2. In the benchmark branch, roll the code back to before the change
   (`git checkout <commit>^ -- src/the_file.py`) and keep the new test,
   moved under the hidden folder.
3. Use the original request as the prompt: the issue text, the bug report,
   or the pull request description, trimmed of anything that gives away the
   test. These are real prompts from your project, which is what you want
   the rules to get better at.
4. Check it: the test fails on the rolled back code and passes on the code
   from the commit.

## 5. Why every task needs an automatic check

The loop runs every version of the rules many times (5 tasks x 3 repeats x
2 versions is already 30 answers per generation) and compares pass rates.
Nobody can read all that, so pass or fail must come from a command. An open
ended prompt such as "refactor this module" or "make the README nicer" has
no single right answer and no test, so it cannot be scored, and a rule that
"helps" it cannot be told from one that does not. A normal project prompt
works well as a task as long as you can say, in a test, what done means.

## 6. Pick held out tasks that can move

Mark 2 to 10 tasks `split = "held_out"`. They are run with every version
but never shown to the model that proposes changes, so a rule that only
fits the train tasks is called overfit instead of being kept.

A held out task that always passes or always fails, whatever the rules say,
adds nothing and can block every verdict: in our first Claude run one held
out task passed in all 15 runs and one failed in all 15, so no child could
ever win the held out pairs. After the first root run, look at the per task
results (`bogo -C . export --out tree.json`, or `digest`) and swap
out tasks at 0 of N or N of N. Tasks that fail for a reason a general habit
could fix (a missing argument check, a missing import, an edge case the
docstring names) are the most useful.

## 7. Run it cheaply

1. **Free, mock and replay.** `try.sh` above. For your own tasks, a mock
   needs a `mock.toml` like this one; most people skip to step 2 or 3.
2. **Free, local model.** `--provider local` talks to a model server on your
   machine (see "Local model test" in the README).
3. **Paid, small.** Claude Haiku on your own key:

   ```sh
   export ANTHROPIC_API_KEY=...        # in your shell, never in a file
   bogo evolve --generations 3 --total --seed 7 \
     --provider anthropic --allow-spend --api-model claude-haiku-5-5 \
     --api-no-temperature --api-max-new-tokens 4096 \
     --record replay.jsonl --api-transcript transcript.jsonl
   ```

   - `--total` makes `--generations` the total for the project, so rerunning
     the same command after a timeout never goes past it.
   - `--api-no-temperature` because newer models refuse a temperature.
   - `--record` keeps every answer, so the whole run can be replayed later
     for free with `--provider replay --replay-file replay.jsonl`.
   - `--api-transcript` keeps the provider's token counts, so you can work
     out the real cost.
   - Output cap: Haiku 5.5 thinks before it answers and the thinking counts
     against `--api-max-new-tokens`. In our second run, with a cap of 2,048,
     28 of 243 calls hit it and 14 of them returned nothing but thinking.
     Raise the cap (4,096 for harder tasks); bogo has no switch to
     turn thinking off yet.
   - Worst case cost: calls x (input cap x input price + output cap x output
     price). Calls are about (1 + generations) x repeats x tasks plus one
     mutation call per generation. Our runs: 16 tasks, 15 runs and 243
     calls cost $0.14; the worst case was about $0.32.

When a version is accepted, `bogo apply --yes` writes it into your
rules file.

## 8. Windows

- Checks run with `sh`. Install Git for Windows (it brings `sh.exe`) or use
  WSL2. `try.sh` runs from Git Bash.
- Isolation: Docker Desktop or Podman Desktop (bwrap is Linux only). Runs
  never pull images, so pull one that has what your checks need first, and
  set it in `config.toml`. For the Python sample:

  ```sh
  docker pull python:3.12-alpine
  ```

  ```toml
  [sandbox]
  isolation = "docker"
  image = "python:3.12-alpine"
  ```

  The default `alpine:3.20` has only `sh`. Inside the container the command
  is `python3`, as in the sample checks.
- `ISOLATION=none examples/starter/try.sh` runs the sample without a
  container (allowed only for the mock and replay providers, never for a
  real model).
- See `packaging/windows/WINDOWS-SETUP.md` for the full setup.
