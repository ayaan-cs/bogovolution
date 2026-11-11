---
name: export
description: Export the whole rules-evolve lineage as one JSON file for the static viewer. Writes one file, no model call.
argument-hint: "[output-path]"
disable-model-invocation: true
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/re.sh *)
---

Export the lineage for the repo in the current directory.

Output path: `$ARGUMENTS` if the user gave one, otherwise `rules-evolve-lineage.json` in the current directory. Never use `-` (that prints the whole export into the chat).

Run:

```
${CLAUDE_PLUGIN_ROOT}/scripts/re.sh export --out <path> --json
```

The export is only written when the log verifies. If the command fails because the log is broken, say so and suggest `/rules-evolve:verify-log`.

On success, report the path, the file size, and the number of versions and edges if the JSON output gives them. Then tell the user how to look at it: open the static viewer in the rules-evolve repo (`viewer/`, see the README section "Viewer") and load the file there. Nothing is uploaded anywhere.

Do not call any model.
