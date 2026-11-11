# Shared helpers for the rules-evolve plugin scripts. Sourced, not run.
# POSIX sh. jq is used when present, with a plain sed fallback.

# True (0) when the hooks are switched off.
#   RULES_EVOLVE_HOOKS=off|0|false|no   turns them off everywhere
#   <repo>/.rules-evolve/hooks.off      turns them off for one repo ($1 = repo root)
re_hooks_disabled() {
  case "${RULES_EVOLVE_HOOKS:-}" in
    off | OFF | 0 | false | FALSE | no | NO) return 0 ;;
  esac
  [ -n "${1:-}" ] && [ -e "$1/.rules-evolve/hooks.off" ] && return 0
  return 1
}

# Print the path of the rules-evolve binary, or return 1 when there is none.
# Order: $RULES_EVOLVE_BIN, PATH, then a cargo build next to this plugin
# (integrations/claude-code/plugin/scripts -> repo root).
re_find_bin() {
  if [ -n "${RULES_EVOLVE_BIN:-}" ]; then
    if [ -x "$RULES_EVOLVE_BIN" ]; then
      printf '%s\n' "$RULES_EVOLVE_BIN"
      return 0
    fi
    return 1
  fi
  _re_found=$(command -v rules-evolve 2>/dev/null) || _re_found=""
  if [ -n "$_re_found" ] && [ -x "$_re_found" ]; then
    printf '%s\n' "$_re_found"
    return 0
  fi
  _re_here=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || return 1
  for _re_cand in \
    "$_re_here/../../../../target/release/rules-evolve" \
    "$_re_here/../../../../target/debug/rules-evolve"; do
    if [ -x "$_re_cand" ]; then
      printf '%s\n' "$_re_cand"
      return 0
    fi
  done
  return 1
}

# Walk up from directory $1 and print the first folder that holds .rules-evolve/.
re_find_root() {
  _re_dir=$1
  _re_n=0
  while [ -n "$_re_dir" ] && [ "$_re_n" -lt 25 ]; do
    if [ -d "$_re_dir/.rules-evolve" ]; then
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
