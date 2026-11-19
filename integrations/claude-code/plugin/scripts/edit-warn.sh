#!/bin/sh
# PostToolUse hook for Edit, Write and MultiEdit.
# Warns (never blocks) when the agent edits the repo's rules file directly,
# outside bogo. Watches the rules_file from .bogo/config.toml plus
# AGENTS.md, CLAUDE.md and .cursor/rules at the repo root.
# Exits 0 and prints nothing when: hooks are off, the file is not a rules file,
# the repo has no .bogo/, or the same file already warned in this session.
. "$(cd "$(dirname "$0")" && pwd)/lib.sh"

input=$(cat 2>/dev/null) || input=""
file=$(printf '%s' "$input" | json_field file_path)
[ -n "$file" ] || exit 0

cwd=$(printf '%s' "$input" | json_field cwd)
[ -n "$cwd" ] || cwd=${CLAUDE_PROJECT_DIR:-$PWD}
case "$file" in
  /*) ;;
  *) file="$cwd/$file" ;;
esac

root=$(re_find_root "$(dirname "$file")") || exit 0
re_hooks_disabled "$root" && exit 0

rel=${file#"$root"/}
[ "$rel" != "$file" ] || exit 0

configured=""
if [ -f "$root/.bogo/config.toml" ]; then
  configured=$(sed -n -E 's/^[[:space:]]*rules_file[[:space:]]*=[[:space:]]*"([^"]*)".*/\1/p' \
    "$root/.bogo/config.toml" | head -n 1)
fi

hit=""
[ -n "$configured" ] && [ "$rel" = "$configured" ] && hit=configured
if [ -z "$hit" ]; then
  case "$rel" in
    AGENTS.md | CLAUDE.md | .cursor/rules | .cursor/rules/*) hit=candidate ;;
  esac
fi
[ -n "$hit" ] || exit 0

# Warn once per file per session so a long edit session is not nagged.
session=$(printf '%s' "$input" | json_field session_id | tr -c 'A-Za-z0-9_-' '_')
if [ -n "$session" ]; then
  state="${CLAUDE_PLUGIN_DATA:-${TMPDIR:-/tmp}}/bogo-edit-warned"
  mark="$state/$session.$(printf '%s' "$rel" | tr -c 'A-Za-z0-9_-' '_')"
  [ -e "$mark" ] && exit 0
  mkdir -p "$state" 2>/dev/null && : >"$mark" 2>/dev/null
fi

if [ "$hit" = configured ]; then
  what="$rel is the rules file tracked by bogo"
else
  what="$rel is an agent rules file"
fi
user="bogo warning: $what and was edited directly. That change is not recorded or benchmarked. To record a measured change use /bogo:propose. To silence this, set BOGO_HOOKS=off."
ctx="$what, and it was edited directly outside bogo, so the lineage in .bogo/ does not include this change. A measured change goes through /bogo:propose. This is a warning only, nothing was blocked."

printf '{"systemMessage":"%s","hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"%s"}}\n' \
  "$(printf '%s' "$user" | json_escape)" \
  "$(printf '%s' "$ctx" | json_escape)"
exit 0
