---
name: status
description: Show the graft lineage status for this repo: log integrity, number of versions, accepted, rejected and noise mutations, and the current version. Read only, no model call.
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Show the graft status for the repo in the current directory.

Run this one command and read its JSON:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh status --json
```

Exit code 0 means the log is intact. Exit code 2 means the command ran but the log failed verification. Exit code 1 means an error (for example the repo has no `.graft/` yet: tell the user to run `/graft:init`). Exit code 127 means the binary was not found: show the message the wrapper printed.

Present the result briefly and plainly:

1. One line for log health (`valid`, `chain.events`) and the rules file and model name.
2. One line of counts: versions, accepted, rejected, noise, pending, refused (from `tree`).
3. The current version as its first 12 hex characters, and say whether it differs from the root.
4. A short table of the edges: parent short hash, child short hash, verdict, and the hypothesis.

If `valid` is false, say so first, list `chain.problems` and `blob_problems`, and suggest `/graft:verify-log`. Do not edit anything under `.graft/` by hand and do not try to repair the log.

Do not call any model and do not run any other graft command.
