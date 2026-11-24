# Shared helpers for the graft plugin scripts. Sourced, not run.
# POSIX sh. jq is used when present, with a plain sed fallback.

# True (0) when the hooks are switched off.
#   GRAFT_HOOKS=off|0|false|no   turns them off everywhere
#   <repo>/.graft/hooks.off      turns them off for one repo ($1 = repo root)
# The older names still work when GRAFT_HOOKS is unset: BOGO_HOOKS (3.x)
# and RULES_EVOLVE_HOOKS (2.x). A 3.x repo keeps its files in .bogo/.
re_hooks_disabled() {
  case "${GRAFT_HOOKS:-${BOGO_HOOKS:-${RULES_EVOLVE_HOOKS:-}}}" in
    off | OFF | 0 | false | FALSE | no | NO) return 0 ;;
  esac
  [ -n "${1:-}" ] && [ -e "$1/$(re_project_dir "$1")/hooks.off" ] && return 0
  return 1
}

# Print the project folder name for repo root $1: .graft, or .bogo when a
# repo from 3.x has only that (graft reads it in place).
re_project_dir() {
  if [ ! -e "$1/.graft" ] && [ -d "$1/.bogo" ]; then
    printf '%s\n' .bogo
  else
    printf '%s\n' .graft
  fi
}

# Print the path of the graft binary, or return 1 when there is none.
# Order: $GRAFT_BIN, PATH, then a cargo build next to this plugin
# (integrations/claude-code/plugin/scripts -> repo root).
# The older names still work when GRAFT_BIN is unset: BOGO_BIN (3.x) and
# RULES_EVOLVE_BIN (2.x).
re_find_bin() {
  if [ -z "${GRAFT_BIN:-}" ] && [ -n "${BOGO_BIN:-}" ]; then
    GRAFT_BIN=$BOGO_BIN
  fi
  if [ -z "${GRAFT_BIN:-}" ] && [ -n "${RULES_EVOLVE_BIN:-}" ]; then
    GRAFT_BIN=$RULES_EVOLVE_BIN
  fi
  if [ -n "${GRAFT_BIN:-}" ]; then
    if [ -x "$GRAFT_BIN" ]; then
      printf '%s\n' "$GRAFT_BIN"
      return 0
    fi
    return 1
  fi
  _re_found=$(command -v graft 2>/dev/null) || _re_found=""
  if [ -n "$_re_found" ] && [ -x "$_re_found" ]; then
    printf '%s\n' "$_re_found"
    return 0
  fi
  _re_here=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || return 1
  for _re_cand in \
    "$_re_here/../../../../target/release/graft" \
    "$_re_here/../../../../target/debug/graft"; do
    if [ -x "$_re_cand" ]; then
      printf '%s\n' "$_re_cand"
      return 0
    fi
  done
  return 1
}

# Walk up from directory $1 and print the first folder that holds .graft/
# (or .bogo/ from 3.x).
re_find_root() {
  _re_dir=$1
  _re_n=0
  while [ -n "$_re_dir" ] && [ "$_re_n" -lt 25 ]; do
    if [ -d "$_re_dir/.graft" ] || [ -d "$_re_dir/.bogo" ]; then
      printf '%s\n' "$_re_dir"
      return 0
    fi
    [ "$_re_dir" = "/" ] && break
    _re_parent=$(dirname "$_re_dir")
    [ "$_re_parent" = "$_re_dir" ] && break
    _re_dir=$_re_parent
    _re_n=$((_re_n + 1))
  done
  return 1
}

# json_field KEY < json : print the first string value of KEY, or nothing.
# Looks for a top level or nested key by name only (cwd, session_id, source, file_path).
json_field() {
  _re_key=$1
  _re_in=$(cat)
  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$_re_in" | jq -r --arg k "$_re_key" \
      '[.. | objects | select(has($k)) | .[$k] | select(type == "string")] | first // empty' 2>/dev/null
    return 0
  fi
  printf '%s' "$_re_in" | tr '\n' ' ' |
    sed -n -E "s/.*\"$_re_key\"[[:space:]]*:[[:space:]]*\"(([^\"\\\\]|\\\\.)*)\".*/\1/p" |
    sed -e 's/\\"/"/g' -e 's/\\\\/\\/g' | head -n 1
}

# json_escape: read stdin, print it as the inside of a JSON string.
json_escape() {
  awk 'BEGIN { ORS = "" }
    {
      gsub(/\\/, "\\\\"); gsub(/"/, "\\\""); gsub(/\t/, "\\t"); gsub(/\r/, "")
      if (NR > 1) print "\\n"
      print
    }'
}

# Run a command with a time limit when `timeout` exists.
re_timeout() {
  _re_secs=$1
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout "$_re_secs" "$@"
  else
    "$@"
  fi
}
