---
name: verify-log
description: Check the rules-evolve log hash chain and stored rules versions for tampering or cut off data. Read only.
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Verify the rules-evolve log for the repo in the current directory.

Run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh verify-log --json
```

Exit code 0 means the chain is intact and every stored file matches its hash. Exit code 2 means verification failed.

Report one clear verdict first: "intact" with the event count and head sequence number, or "FAILED" with each problem in `chain.problems` and `blob_problems` (kind and detail). On failure, say what it means in one sentence (for example a line was changed, or the newest entries were cut off) and tell the user to compare against git history of `.rules-evolve/`. Do not edit or delete anything under `.rules-evolve/` and do not try to repair the log.

Do not call any model.
