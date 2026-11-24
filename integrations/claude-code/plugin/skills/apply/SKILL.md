---
name: apply
description: Copy an accepted graft version into the repo's real rules file (AGENTS.md, CLAUDE.md or .cursor/rules). Shows the diff first and writes only after the user confirms. No model is called.
argument-hint: "[version, default current]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Write an accepted version from the graft lineage into the repo's rules file. This is the one command that changes the real rules file, so it always shows the diff and asks first.

The user's request: `$ARGUMENTS` (a version hash or prefix; empty means `current`, the newest accepted version).

Steps:

1. Dry run, which writes nothing:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh apply --version <version or current> --json
```

If `data.outcome` is `unchanged`, say the rules file already holds that version and stop. If the command fails because the version is not accepted, show the message and stop. Do not add `--force` on your own; only pass it if the user's request above explicitly asks to force a version that was not accepted, and then say clearly what its status is.

2. Show the user `data.diff`, the short hash (12 characters) and `data.status`, and ask them to confirm. Stop here unless they say yes in their next message.

3. After a clear yes, write it:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh apply --version <the same full hash> --yes --json
```

4. Report `data.outcome` (`written`), the rules file and `data.event_seq` (the `applied` event in the log). Remind the user to review and commit the rules file with git.

Never edit the rules file directly. Do not call any model. Do not pass any provider option.
