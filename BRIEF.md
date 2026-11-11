# Project brief: evolving repo agent rules (working title, unnamed)

Inspired by Petri (https://ethglobal.com/showcase/petri-mjy1y), ETHOnline 2026. Credit it as inspiration; the angle here is different.

## The problem
Developers tweak AGENTS.md, CLAUDE.md, .cursor/rules, custom prompts and MCP tool descriptions on gut feel. Nobody measures whether a change helped, and failed ideas get retried.

## What it does
The model stays fixed. The rules file evolves. Each change is a small mutation of its parent, benchmarked on tasks from the user's own repo, re-run several times, and kept only when it beats its parent by a clear margin. Every attempt, including failures and refused mutations, stays in a lineage tree.

## v1: local CLI (bring your own API key)
- `init`: pick the rules file, the model, and a small task set from the repo (for example failing tests or past issues with known fixes).
- `mutate`: propose a small, size-limited change with a plain-English hypothesis.
- `run` / `verify`: run parent and child several times each, use the median, then accept, reject, or mark as noise by a fixed public rule.
- `digest`: turn the tree into a brief of what worked, what failed and what is untried.
- `--json` output on every command so scripts and the later plugin can call it.
- Replay mode: recorded model answers so demos and the viewer run with no key and no cost.
- Append-only, hash-chained log; tree state rebuilt from it.
- Sandbox the harness so it can never read the benchmark tests.
- Language: Rust is the likely pick (fits aco and Ayaan's Rust learning).

## v1.1: ratatui watch view
- Optional `watch` TUI: live mutation tree, run progress, verdicts as verifiers finish. Built on the same engine, not part of it.

## Static viewer
- Reads an exported tree (JSON). No keys, no server. Lineage graph, edge labels like "accuracy +40%", per-generation chart. Can be linked from the portfolio.

## v2
- Claude Code plugin (hooks, slash commands). VS Code extension only if v1 gets real use.

## Not doing
- No paste-your-API-key website.
- Not claiming to evolve model weights.

## Open questions
- Name.
- How tasks are defined per repo; acceptance threshold and repeat count.
- Token budget per run; stay within included usage, no out-of-pocket charges.
