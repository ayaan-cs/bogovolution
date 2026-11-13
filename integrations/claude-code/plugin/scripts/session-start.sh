#!/bin/sh
# SessionStart hook: show a one line rules-evolve status when the repo has .rules-evolve/.
# Read only (runs `rules-evolve status`), makes no model call, never blocks.
# Exits 0 and prints nothing when: hooks are off, the repo has no .rules-evolve/,
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

summary="rules-evolve: log ${log:-unknown}"
[ -n "$versions" ] && summary="$summary; versions $versions"
[ -n "$mutations" ] && summary="$summary; mutations $mutations"
[ -n "$current" ] && summary="$summary; current $current"

context="This repo uses rules-evolve (lineage in .rules-evolve/). Status: ${summary#rules-evolve: }. Slash commands: /rules-evolve:status, /rules-evolve:digest, /rules-evolve:verify-log, /rules-evolve:propose, /rules-evolve:evolve, /rules-evolve:export, /rules-evolve:apply. The rules file is changed through rules-evolve propose and apply, not by editing it directly."
if [ "$code" = "2" ]; then
  context="$context The log failed verification: do not edit files under .rules-evolve/ by hand, and run /rules-evolve:verify-log."
  summary="$summary. Run /rules-evolve:verify-log."
fi

printf '{"systemMessage":"%s","hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' \
  "$(printf '%s' "$summary" | json_escape)" \
  "$(printf '%s' "$context" | json_escape)"
exit 0
