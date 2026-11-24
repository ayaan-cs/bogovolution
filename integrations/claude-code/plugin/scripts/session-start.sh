#!/bin/sh
# SessionStart hook: show a one line graft status when the repo has .graft/.
# Read only (runs `graft status`), makes no model call, never blocks.
# Exits 0 and prints nothing when: hooks are off, the repo has no .graft/,
# or the binary is missing.
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

input=$(cat 2>/dev/null) || input=""
cwd=$(printf '%s' "$input" | json_field cwd)
[ -n "$cwd" ] || cwd=${CLAUDE_PROJECT_DIR:-$PWD}

root=$(re_find_root "$cwd") || exit 0
re_hooks_disabled "$root" && exit 0
bin=$(re_find_bin) || exit 0

out=$(re_timeout 10 "$bin" -C "$root" status 2>/dev/null)
code=$?
# 0 = ok, 2 = ran but the log failed verification. Anything else: stay quiet.
case "$code" in 0 | 2) ;; *) exit 0 ;; esac
[ -n "$out" ] || exit 0

pick() { printf '%s\n' "$out" | sed -n "s/^$1: //p" | head -n 1; }
log=$(pick Log)
versions=$(pick Versions)
mutations=$(pick Mutations)
current=$(pick Current)

summary="graft: log ${log:-unknown}"
[ -n "$versions" ] && summary="$summary; versions $versions"
[ -n "$mutations" ] && summary="$summary; mutations $mutations"
[ -n "$current" ] && summary="$summary; current $current"

context="This repo uses graft (lineage in .graft/). Status: ${summary#graft: }. Slash commands: /graft:status, /graft:digest, /graft:verify-log, /graft:propose, /graft:evolve, /graft:export, /graft:apply. The rules file is changed through graft propose and apply, not by editing it directly."
if [ "$code" = "2" ]; then
  context="$context The log failed verification: do not edit files under .graft/ by hand, and run /graft:verify-log."
  summary="$summary. Run /graft:verify-log."
fi

printf '{"systemMessage":"%s","hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' \
  "$(printf '%s' "$summary" | json_escape)" \
  "$(printf '%s' "$context" | json_escape)"
exit 0
