---
name: propose
description: Record one small, measurable change to the rules file as a new child version in the rules-evolve lineage, with a plain English hypothesis. Does not edit the rules file itself.
argument-hint: "[what to change and why]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Record a proposed change to the rules file through rules-evolve, so it can be benchmarked. Never edit the rules file (`AGENTS.md`, `CLAUDE.md` or `.cursor/rules`) directly for this: that bypasses the lineage.

The user's request: `$ARGUMENTS`

Steps:

1. Run `${CLAUDE_PLUGIN_ROOT}/scripts/re.sh status --json`. Note `tree.rules_file`, `tree.current` and `config.mutation.max_change_bytes` if present (default limit is 1000 bytes of change). If there is no `.rules-evolve/`, tell the user to run `/rules-evolve:init` and stop.
2. Read the text of the current version. The full hash is `tree.current`, and the text is the file `.rules-evolve/blobs/<full hash>`.
3. Make ONE small change to that text that fits the user's request, and write the complete new rules text to `.rules-evolve/scratch/proposal.md` (that folder is gitignored). If the request is empty or too vague, ask one short question instead of guessing.
4. Write a one sentence hypothesis saying why the change should help.
5. Run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh propose --file .rules-evolve/scratch/proposal.md --hypothesis "<hypothesis>" --json
```

Exit code 0 means the child version was recorded (`data.child`, `data.change_bytes`). Exit code 3 means the change was refused and the refusal was logged (`data.reason`): show the reason and offer a smaller change. Exit code 1 is an error: show it.

6. Report the child short hash (12 characters), the size of the change, and the hypothesis. Say it is not benchmarked yet. Next steps are `rules-evolve run` and `verify` for that child, or `/rules-evolve:evolve` for the automatic loop. Once a version is accepted, `/rules-evolve:apply` writes it to the real rules file after the user confirms.

Do not call any model. Do not pass any provider option.
