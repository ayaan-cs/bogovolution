---
name: digest
description: Plain English brief of the rules-evolve lineage: what worked, what failed, what is noise, what is pending, and directions not tried yet. Read only, no model call.
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Give a plain English brief of the rules-evolve lineage for the repo in the current directory.

Run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh digest
```

The text output is already written as a brief. Show it with light formatting: keep its sections (what worked, what failed, noise, pending, directions not tried) and do not invent results that are not in the output. If the command fails, show the error line and, for a missing `.rules-evolve/`, suggest `/rules-evolve:init`.

If the user wants the structured form, run `${CLAUDE_PLUGIN_ROOT}/scripts/re.sh digest --json` instead.

Do not call any model. Do not run other rules-evolve commands.
