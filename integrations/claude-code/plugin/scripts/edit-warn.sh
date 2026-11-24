#!/bin/sh
# PostToolUse hook for Edit, Write and MultiEdit.
# Warns (never blocks) when the agent edits the repo's rules file directly,
# outside graft. Watches the rules_file from .graft/config.toml plus
# AGENTS.md, CLAUDE.md and .cursor/rules at the repo root.
# Exits 0 and prints nothing when: hooks are off, the file is not a rules file,
# the repo has no .graft/, or the same file already warned in this session.
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
cfg="$root/$(re_project_dir "$root")/config.toml"
if [ -f "$cfg" ]; then
  configured=$(sed -n -E 's/^[[:space:]]*rules_file[[:space:]]*=[[:space:]]*"([^"]*)".*/\1/p' \
    "$cfg" | head -n 1)
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
  state="${CLAUDE_PLUGIN_DATA:-${TMPDIR:-/tmp}}/graft-edit-warned"
  mark="$state/$session.$(printf '%s' "$rel" | tr -c 'A-Za-z0-9_-' '_')"
  [ -e "$mark" ] && exit 0
  mkdir -p "$state" 2>/dev/null && : >"$mark" 2>/dev/null
fi

if [ "$hit" = configured ]; then
  what="$rel is the rules file tracked by graft"
else
  what="$rel is an agent rules file"
fi
user="graft warning: $what and was edited directly. That change is not recorded or benchmarked. To record a measured change use /graft:propose. To silence this, set GRAFT_HOOKS=off."
ctx="$what, and it was edited directly outside graft, so the lineage in .graft/ does not include this change. A measured change goes through /graft:propose. This is a warning only, nothing was blocked."

printf '{"systemMessage":"%s","hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"%s"}}\n' \
  "$(printf '%s' "$user" | json_escape)" \
  "$(printf '%s' "$ctx" | json_escape)"
exit 0
