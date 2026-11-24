---
name: init
description: Set up .graft/ in this repo (config, task template, lineage log) so graft can track the rules file. Writes files, makes no model call.
argument-hint: "[model-name] [--rules-file PATH]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Set up graft in the repo in the current directory.

Arguments: `$ARGUMENTS`

The model name is only recorded, never called. Use the first plain word in the arguments as the model name. If there is none, ask the user which model name to record (for example the model they use with their agent) and stop.

First check whether `.graft/` already exists. If it does, say so and show `${CLAUDE_PLUGIN_ROOT}/scripts/re.sh status --json` instead of running init.

Otherwise run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh init --model <name> [--rules-file <path>] --json
```

Report: which rules file was detected (`rules_file`, and `other_candidates` if several), the files created, any `warnings`, and the `gitignore` status. Remind the user that `.graft/` is meant to be committed to git, and that the task template in `.graft/tasks.toml` needs real tasks (a prompt plus a shell check) before a benchmark means anything. Do not commit anything yourself.
