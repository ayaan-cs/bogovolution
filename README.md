# rules-evolve (working name)

A small command line tool that helps you improve your repo's agent rules file
(`AGENTS.md`, `CLAUDE.md`, or a file in `.cursor/rules`) with measurements
instead of gut feeling.

The idea:

- The model stays fixed. Only the rules file changes.
- Each change is a small mutation of its parent version, with a plain-English
  hypothesis ("tell the agent to run the tests first").
- Each version is benchmarked on tasks taken from your own repo, several times,
  and the median is used.
- A change is kept only if it clearly beats its parent. Otherwise it is
  rejected or marked as noise.
- Every attempt, including failures and refused changes, is written to an
  append-only, hash-chained log. The lineage tree is rebuilt from that log.

It is bring your own key and runs locally. Nothing is uploaded anywhere.

## Inspiration

This project is inspired by [Petri](https://ethglobal.com/showcase/petri-mjy1y)
(ETHOnline 2026). Petri is the inspiration only. The angle here is different:
a local tool that evolves a repo's rules file against that repo's own tasks.

## Status: Phase 1

Phase 1 builds the foundation and makes **no model calls**. It has no
benchmark harness, no `mutate`, no viewer, and no TUI yet.

Done in Phase 1:

- Cargo workspace: `crates/core` (engine library) and `crates/cli` (the
  `rules-evolve` binary).
- Append-only, hash-chained log with tamper and truncation checks.
- Tree rebuilt from the log. Rules file versions are stored by content hash.
- Commands: `init`, `status`, `log`, `verify-log`. All support `--json`.

Planned later (not built yet): `mutate`, `run` / `verify`, `digest`, replay
mode, a static JSON lineage viewer, an optional `watch` terminal view, and a
Claude Code plugin.

## Not doing

- No paste-your-API-key website. Keys stay on your machine.
- No claim about evolving model weights. The model never changes. Only the
  rules text evolves.

## Build and test

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The binary is `target/debug/rules-evolve`.

## Commands

Global options: `--json` (machine readable output) and `-C, --dir <DIR>` (repo
root, default `.`).

### `init --model <NAME> [--rules-file <PATH>]`

Creates `.rules-evolve/` in the repo. If `--rules-file` is not given, it looks
for `AGENTS.md`, then `CLAUDE.md`, then `.cursor/rules` and uses the first one
it finds. A `.cursor/rules` folder is used only if it holds exactly one file,
since one file is evolved at a time. The model name is only recorded. No model
is called.

It writes:

| Path | What it is |
| --- | --- |
| `.rules-evolve/config.toml` | Settings (see below) |
| `.rules-evolve/tasks.toml` | Task template: each task is a prompt plus a shell check |
| `.rules-evolve/log.jsonl` | The lineage log, starting with an `init` event |
| `.rules-evolve/HEAD` | Newest `seq` and `hash`, used to spot cut off logs |
| `.rules-evolve/blobs/<sha256>` | Rules file versions, named by content hash |

Default settings (confirmed by the project owner, and recorded in `config.toml`
as `defaults_confirmed = true`):

| Setting | Default | Meaning |
| --- | --- | --- |
| `repeats` | 5 | Runs per version, median taken |
| `accept_threshold_pp` | 10 | Child must beat parent by 10 percentage points of median pass rate |
| `min_paired_wins` | 4 | And win at least 4 of the 5 paired runs, otherwise it is noise |
| `max_tokens_per_run` | 200000 | Token cap for one run |

A task in `tasks.toml` looks like this. The check command exits with 0 to pass.

```toml
[[task]]
id = "example-fix-failing-test"
prompt = "The test suite has one failing test. Fix the code so all tests pass."
check = "cargo test --quiet"
```

### `status`

Shows whether the log is intact, the settings, and a summary of the tree:
versions, mutations (accepted, rejected, noise, pending), refused mutations,
recorded runs, and each edge with its hypothesis and verdict. If the log fails
verification, the tree is not shown.

### `log [--limit N] [--kind KIND]`

Lists events in order. `--limit N` shows the newest N. `--kind` is one of
`init`, `mutation_proposed`, `mutation_refused`, `run_recorded`, `verdict`.

### `verify-log`

Checks the hash chain, the `HEAD` file, and that every stored rules file
matches its hash.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Error (not initialized, bad arguments, broken log for `log`, and so on) |
| 2 | `status` or `verify-log` ran, but verification failed |

## JSON output

With `--json`, every command prints one JSON object to stdout.

Success:

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "status",
  "data": { }
}
```

Error (also on stdout, with exit code 1):

```json
{
  "schema_version": 1,
  "ok": false,
  "error": { "message": "..." }
}
```

`schema_version` only changes on breaking changes. New fields may be added
without a bump. Object keys are sorted.

`data` per command:

- `init`: `repo_root`, `rules_file`, `detected` (bool), `other_candidates`,
  `notes`, `model`, `root_hash`, `defaults_confirmed` (true),
  `files_created`.
- `status`: `repo_root`, `valid` (chain and blobs both fine), `chain` (see
  below), `blob_problems`, `tree` (or null if the log is invalid), `config`.
  `tree` has `rules_file`, `model`, `root`, `current` (newest accepted
  version, or the root), `nodes`, `edges`, `refused`. A node has `hash`,
  `parent`, `generation`, `created_seq`, `hypothesis`, `verdict`, `runs`. An
  edge has `parent`, `child`, `hypothesis`, `verdict` (null while pending),
  `parent_median`, `child_median`, `paired_wins`, `repeats`, `note`, `seq`.
- `log`: `total`, `shown`, `events` (the raw log events).
- `verify-log`: `valid`, `chain`, `blob_problems`. `chain` has `valid`,
  `events`, `head_seq`, `head_hash`, `problems`. A problem has `code`, `seq`,
  `line`, `message`. Codes: `invalid_line`, `bad_seq`, `bad_prev_hash`,
  `bad_hash`, `head_missing`, `head_mismatch`, `blob_missing`, `blob_corrupt`.

Sample:

```json
{
  "command": "verify-log",
  "data": {
    "blob_problems": [],
    "chain": {
      "events": 1,
      "head_hash": "579448f0...",
      "head_seq": 0,
      "problems": [],
      "valid": true
    },
    "valid": true
  },
  "ok": true,
  "schema_version": 1
}
```

## Log format

`.rules-evolve/log.jsonl` is JSON Lines: one event per line, never edited.
Each event looks like this (shown on several lines here for reading):

```json
{
  "seq": 0,
  "timestamp": "2026-11-11T06:40:06Z",
  "kind": "init",
  "payload": { "rules_file": "AGENTS.md", "model": "demo-model", "root_hash": "1ca6...", "tool_version": "0.1.0" },
  "prev_hash": "0000000000000000000000000000000000000000000000000000000000000000",
  "hash": "5794..."
}
```

- `seq` counts up from 0 with no gaps.
- `timestamp` is UTC, RFC 3339, whole seconds.
- `prev_hash` is the `hash` of the previous event. The first event uses 64 zeros.
- `hash` is the SHA-256 (lowercase hex) of the canonical JSON of
  `{seq, timestamp, kind, payload, prev_hash}`. Canonical means compact, with
  object keys sorted, and strings escaped the standard JSON way. Because
  `prev_hash` is included, each event commits to everything before it.

Event kinds (names are stable):

| Kind | Payload fields | Meaning |
| --- | --- | --- |
| `init` | `rules_file`, `model`, `root_hash`, `tool_version` | Start of the lineage. Always `seq` 0. |
| `mutation_proposed` | `parent`, `child`, `hypothesis` | A child version of a parent version |
| `mutation_refused` | `parent`, `hypothesis` (optional), `reason` | A change that was not allowed, for example too large |
| `run_recorded` | `node`, `run_index`, `passed`, `total`, `tokens_used` | One benchmark run of one version |
| `verdict` | `parent`, `child`, `verdict`, `parent_median`, `child_median`, `paired_wins`, `repeats`, `note` | `verdict` is `accepted`, `rejected`, or `noise` |

In Phase 1 only `init` is written by a command. The other kinds are defined,
checked, and tested in the engine so later phases can use them.

### What the checks catch

- Editing any event: its hash no longer matches (`bad_hash`).
- Editing an event and fixing its hash: the next event's `prev_hash` no longer
  matches (`bad_prev_hash`).
- Removing or reordering events: the `seq` numbers no longer line up (`bad_seq`).
- A half written last line: `invalid_line`.
- Cutting events off the end: the `HEAD` file still names the old end
  (`head_mismatch`). If `HEAD` is deleted, that is reported too (`head_missing`).
- A changed or deleted stored rules file: `blob_corrupt` or `blob_missing`.

Limit: this is tamper evident, not tamper proof. Someone who rewrites the whole
log and `HEAD` together can produce a chain that checks out. Keep the log in git
to get a history outside the tool.

### The tree

The tree is never stored on its own. It is rebuilt from the log each time:
nodes are rules file versions (named by content hash), edges go from parent to
child with the hypothesis and verdict. The rebuild is strict. It rejects, for
example, a child with an unknown parent, a version that already exists, a
second verdict for the same child, or a log that does not start with `init`. The
engine runs the same checks before it appends a new event.

## Cost

Phase 1 makes no network calls and costs nothing to run. Later phases will use
your own API key, and a per run token cap (default 200000) is part
of the config from the start.
