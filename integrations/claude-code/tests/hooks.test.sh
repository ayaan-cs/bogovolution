#!/bin/sh
# Tests for the plugin scripts (hooks and the re.sh wrapper).
# Mock and replay only: the real binary is used only on the committed demo
# replay file. No network, no model.
#
#   tests/hooks.test.sh                 builds nothing, uses target/release or target/debug if present
#   RULES_EVOLVE_BIN=/path tests/hooks.test.sh
#
# Every hook case runs twice: once with jq on PATH and once with a PATH that has no jq,
# so the sed fallback is covered too.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
PLUGIN=$(cd "$HERE/../plugin" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
SCRIPTS=$PLUGIN/scripts

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); }
bad() {
  FAIL=$((FAIL + 1))
  printf 'FAIL [%s]: %s\n' "${MODE:-}" "$1" >&2
}
assert_eq() { [ "$2" = "$3" ] && ok || bad "$1: expected '$3', got '$2'"; }
assert_empty() { [ -z "$2" ] && ok || bad "$1: expected no output, got '$2'"; }
assert_contains() {
  case "$2" in *"$3"*) ok ;; *) bad "$1: expected to contain '$3', got '$2'" ;; esac
}
assert_not_contains() {
  case "$2" in *"$3"*) bad "$1: must not contain '$3', got '$2'" ;; *) ok ;; esac
}
assert_json() {
  if printf '%s' "$2" | node -e 'JSON.parse(require("fs").readFileSync(0,"utf8"))' 2>/dev/null; then ok; else bad "$1: not valid JSON: $2"; fi
}
# json_get '<expr over j>' < json
json_get() { node -e "const j=JSON.parse(require('fs').readFileSync(0,'utf8')); const v=($1); process.stdout.write(String(v))"; }

# A PATH with the basic tools but no jq.
NOJQ=$TMP/nojq-bin
mkdir -p "$NOJQ"
for t in sh sed awk tr head dirname cat mkdir timeout env date basename rm; do
  p=$(command -v "$t" 2>/dev/null) && [ -n "$p" ] && ln -sf "$p" "$NOJQ/$t"
done

# Real binary, if one is built.
REAL=${RULES_EVOLVE_BIN:-}
if [ -z "$REAL" ]; then
  for c in "$REPO/target/release/rules-evolve" "$REPO/target/debug/rules-evolve"; do
    [ -x "$c" ] && REAL=$c && break
  done
fi

# Fake binary: prints a canned `status`, exit code from FAKE_EXIT (default 0).
FAKE=$TMP/fake/rules-evolve
mkdir -p "$TMP/fake"
cat >"$FAKE" <<'EOS'
#!/bin/sh
# records its arguments in $FAKE_LOG when set
[ -n "${FAKE_LOG:-}" ] && printf '%s\n' "$*" >>"$FAKE_LOG"
case "$*" in
  *status*)
    printf 'Log: intact (12 events)\nSettings: x\nRules file: AGENTS.md\nModel: m\nRoot: aaaaaaaaaaaa\nCurrent: bbbbbbbbbbbb\nVersions: 3 (deepest generation 2)\nMutations: 2 (accepted 1, rejected 1, noise 0, pending 0)\n'
    ;;
  *) printf 'fake ran: %s\n' "$*" ;;
esac
exit "${FAKE_EXIT:-0}"
EOS
chmod +x "$FAKE"

# demo_repo DEST: a copy of the demo lineage (built once with the committed replay file)
demo_repo() {
  if [ ! -d "$TMP/demo-base" ]; then
    DEMO_DIR=$TMP/demo-base RULES_EVOLVE=$REAL sh "$REPO/examples/demo/demo.sh" >/dev/null 2>&1
  fi
  cp -R "$TMP/demo-base" "$1"
}

# mkrepo NAME -> a folder with .rules-evolve/config.toml and rules files
mkrepo() {
  d=$TMP/$1
  mkdir -p "$d/.rules-evolve" "$d/src" "$d/.cursor/rules"
  printf '[project]\nrules_file = "%s"\nmodel = "m"\n' "${2:-AGENTS.md}" >"$d/.rules-evolve/config.toml"
  : >"$d/AGENTS.md"
  : >"$d/CLAUDE.md"
  printf '%s' "$d"
}

# run_hook SCRIPT 'stdin json' [env assignments...] -> sets OUT and CODE
run_hook() {
  _s=$1
  _in=$2
  shift 2
  if [ "$MODE" = nojq ]; then
    OUT=$(printf '%s' "$_in" | env PATH="$NOJQ" "$@" "$SCRIPTS/$_s" 2>"$TMP/stderr")
  else
    OUT=$(printf '%s' "$_in" | env "$@" "$SCRIPTS/$_s" 2>"$TMP/stderr")
  fi
  CODE=$?
}

session_in() { printf '{"session_id":"s1","cwd":"%s","hook_event_name":"SessionStart","source":"startup"}' "$1"; }
edit_in() { printf '{"session_id":"%s","cwd":"%s","hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"%s","old_string":"a","new_string":"b"}}' "$3" "$1" "$2"; }

test_session_start() {
  # no .rules-evolve: silent
  e=$TMP/empty-$MODE
  mkdir -p "$e"
  run_hook session-start.sh "$(session_in "$e")" RULES_EVOLVE_BIN="$FAKE"
  assert_eq "no .rules-evolve exit" "$CODE" 0
  assert_empty "no .rules-evolve output" "$OUT"

  # fake binary: summary
  r=$(mkrepo "ss-$MODE")
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$FAKE"
  assert_eq "summary exit" "$CODE" 0
  assert_json "summary json" "$OUT"
  msg=$(printf '%s' "$OUT" | json_get 'j.systemMessage')
  assert_contains "summary text log" "$msg" "log intact (12 events)"
  assert_contains "summary text versions" "$msg" "versions 3 (deepest generation 2)"
  assert_contains "summary text current" "$msg" "current bbbbbbbbbbbb"
  assert_eq "event name" "$(printf '%s' "$OUT" | json_get 'j.hookSpecificOutput.hookEventName')" SessionStart
  ctx=$(printf '%s' "$OUT" | json_get 'j.hookSpecificOutput.additionalContext')
  assert_contains "context lists commands" "$ctx" "/rules-evolve:propose"
  assert_not_contains "no block decision" "$OUT" '"decision"'

  # found from a subfolder
  run_hook session-start.sh "$(session_in "$r/src")" RULES_EVOLVE_BIN="$FAKE"
  assert_contains "subfolder finds root" "$OUT" "log intact"

  # CLAUDE_PROJECT_DIR fallback when stdin has no cwd
  run_hook session-start.sh '{}' RULES_EVOLVE_BIN="$FAKE" CLAUDE_PROJECT_DIR="$r"
  assert_contains "project dir fallback" "$OUT" "log intact"

  # garbage stdin: no crash
  run_hook session-start.sh 'not json at all' RULES_EVOLVE_BIN="$FAKE" CLAUDE_PROJECT_DIR="$e"
  assert_eq "garbage stdin exit" "$CODE" 0
  assert_empty "garbage stdin output" "$OUT"

  # status exit 2 (broken log) is reported with a pointer to verify-log
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$FAKE" FAKE_EXIT=2
  assert_eq "broken log exit" "$CODE" 0
  assert_contains "broken log pointer" "$(printf '%s' "$OUT" | json_get 'j.systemMessage')" "/rules-evolve:verify-log"
  assert_contains "broken log context" "$(printf '%s' "$OUT" | json_get 'j.hookSpecificOutput.additionalContext')" "do not edit files under .rules-evolve/"

  # status exit 1 (error): quiet
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$FAKE" FAKE_EXIT=1
  assert_eq "status error exit" "$CODE" 0
  assert_empty "status error output" "$OUT"

  # binary missing: quiet, exit 0
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$TMP/does-not-exist"
  assert_eq "missing binary exit" "$CODE" 0
  assert_empty "missing binary output" "$OUT"
  assert_empty "missing binary stderr" "$(cat "$TMP/stderr")"

  # binary missing and not on PATH, plugin copied away from any cargo target: quiet
  cp -R "$PLUGIN" "$TMP/plugin-copy-$MODE"
  _save=$SCRIPTS
  SCRIPTS=$TMP/plugin-copy-$MODE/scripts
  run_hook session-start.sh "$(session_in "$r")" PATH="$NOJQ"
  SCRIPTS=$_save
  assert_eq "no binary anywhere exit" "$CODE" 0
  assert_empty "no binary anywhere output" "$OUT"

  # switched off by env and by file
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$FAKE" RULES_EVOLVE_HOOKS=off
  assert_empty "env off" "$OUT"
  : >"$r/.rules-evolve/hooks.off"
  run_hook session-start.sh "$(session_in "$r")" RULES_EVOLVE_BIN="$FAKE"
  assert_empty "file off" "$OUT"
  rm -f "$r/.rules-evolve/hooks.off"

  # a quote and a space in the repo path still gives valid JSON
  q="$TMP/we ird \"q\" $MODE"
  mkdir -p "$q/.rules-evolve"
  run_hook session-start.sh "$(printf '{"cwd":"%s"}' "$(printf '%s' "$q" | sed 's/"/\\"/g')")" RULES_EVOLVE_BIN="$FAKE"
  assert_json "odd path json" "$OUT"
}

test_session_start_real() {
  [ -n "$REAL" ] || { echo "SKIPPED real binary session test (no built binary)" >&2; return 0; }
  w=$TMP/real-$MODE
  demo_repo "$w"
  run_hook session-start.sh "$(session_in "$w")" RULES_EVOLVE_BIN="$REAL"
  assert_eq "real exit" "$CODE" 0
  assert_json "real json" "$OUT"
  msg=$(printf '%s' "$OUT" | json_get 'j.systemMessage')
  assert_contains "real log" "$msg" "log intact (64 events)"
  assert_contains "real versions" "$msg" "versions 9"
  assert_contains "real mutations" "$msg" "accepted 2, rejected 5, noise 1, pending 0"
  assert_contains "real current" "$msg" "current cd28eb296938"

  # tamper: broken log
  cp -R "$w" "$TMP/real-bad-$MODE"
  echo x >>"$TMP/real-bad-$MODE/.rules-evolve/log.jsonl"
  run_hook session-start.sh "$(session_in "$TMP/real-bad-$MODE")" RULES_EVOLVE_BIN="$REAL"
  assert_contains "real broken" "$(printf '%s' "$OUT" | json_get 'j.systemMessage')" "log BROKEN"
}

test_edit_warn() {
  r=$(mkrepo "ew-$MODE")
  D=CLAUDE_PLUGIN_DATA=$TMP/data-$MODE
  rm -rf "$TMP/data-$MODE"

  run_hook edit-warn.sh "$(edit_in "$r" "$r/AGENTS.md" sess1)" "$D"
  assert_eq "warn exit" "$CODE" 0
  assert_json "warn json" "$OUT"
  assert_eq "warn event" "$(printf '%s' "$OUT" | json_get 'j.hookSpecificOutput.hookEventName')" PostToolUse
  um=$(printf '%s' "$OUT" | json_get 'j.systemMessage')
  assert_contains "warn names file" "$um" "AGENTS.md"
  assert_contains "warn says tracked" "$um" "tracked by rules-evolve"
  assert_contains "warn points to propose" "$um" "/rules-evolve:propose"
  assert_contains "warn says how to silence" "$um" "RULES_EVOLVE_HOOKS=off"
  assert_not_contains "warn never blocks" "$OUT" 'permissionDecision'
  assert_not_contains "warn never blocks 2" "$OUT" '"decision"'
  assert_not_contains "warn never stops" "$OUT" '"continue"'

  # same file, same session: only once
  run_hook edit-warn.sh "$(edit_in "$r" "$r/AGENTS.md" sess1)" "$D"
  assert_empty "dedupe same session" "$OUT"
  # another session warns again
  run_hook edit-warn.sh "$(edit_in "$r" "$r/AGENTS.md" sess2)" "$D"
  assert_contains "new session warns" "$OUT" "AGENTS.md"

  # a non rules file is silent
  run_hook edit-warn.sh "$(edit_in "$r" "$r/src/main.rs" sess3)" "$D"
  assert_eq "other file exit" "$CODE" 0
  assert_empty "other file output" "$OUT"
  # a nested AGENTS.md is not the repo rules file
  mkdir -p "$r/src/deep"
  run_hook edit-warn.sh "$(edit_in "$r" "$r/src/deep/AGENTS.md" sess3)" "$D"
  assert_empty "nested AGENTS.md silent" "$OUT"
  # a file inside .rules-evolve is not a rules file either
  run_hook edit-warn.sh "$(edit_in "$r" "$r/.rules-evolve/config.toml" sess3)" "$D"
  assert_empty "config.toml silent" "$OUT"

  # candidates that are not the configured file still warn, with the generic text
  run_hook edit-warn.sh "$(edit_in "$r" "$r/CLAUDE.md" sess4)" "$D"
  assert_contains "CLAUDE.md warns" "$OUT" "CLAUDE.md is an agent rules file"
  run_hook edit-warn.sh "$(edit_in "$r" "$r/.cursor/rules/style.mdc" sess4)" "$D"
  assert_contains "cursor rules warn" "$OUT" ".cursor/rules/style.mdc"

  # a custom configured rules file
  c=$(mkrepo "ewc-$MODE" docs/rules.md)
  mkdir -p "$c/docs"
  run_hook edit-warn.sh "$(edit_in "$c" "$c/docs/rules.md" sess5)" "$D"
  assert_contains "custom rules file warns" "$OUT" "docs/rules.md is the rules file tracked by rules-evolve"

  # Write tool with content that itself mentions file_path must not confuse the parser
  run_hook edit-warn.sh "{\"session_id\":\"sess6\",\"cwd\":\"$r\",\"tool_name\":\"Write\",\"tool_input\":{\"content\":\"{\\\"file_path\\\":\\\"$r/AGENTS.md\\\"}\",\"file_path\":\"$r/src/other.txt\"}}" "$D"
  assert_empty "content mentioning file_path" "$OUT"

  # relative path uses cwd
  run_hook edit-warn.sh "$(edit_in "$r" "AGENTS.md" sess7)" "$D"
  assert_contains "relative path" "$OUT" "AGENTS.md"

  # no .rules-evolve in the repo: silent
  n=$TMP/norules-$MODE
  mkdir -p "$n"
  : >"$n/AGENTS.md"
  run_hook edit-warn.sh "$(edit_in "$n" "$n/AGENTS.md" sess8)" "$D"
  assert_empty "repo without .rules-evolve" "$OUT"

  # switched off
  run_hook edit-warn.sh "$(edit_in "$r" "$r/AGENTS.md" sess9)" "$D" RULES_EVOLVE_HOOKS=off
  assert_empty "env off" "$OUT"
  rm -rf "$TMP/data-$MODE"
  : >"$r/.rules-evolve/hooks.off"
  run_hook edit-warn.sh "$(edit_in "$r" "$r/AGENTS.md" sess10)" "$D"
  assert_empty "file off" "$OUT"
  rm -f "$r/.rules-evolve/hooks.off"

  # works without any rules-evolve binary and without a session id
  run_hook edit-warn.sh "$(printf '{"cwd":"%s","tool_input":{"file_path":"%s/AGENTS.md"}}' "$r" "$r")" RULES_EVOLVE_BIN="$TMP/does-not-exist"
  assert_eq "no binary exit" "$CODE" 0
  assert_contains "no binary still warns" "$OUT" "AGENTS.md"
  run_hook edit-warn.sh "$(printf '{"cwd":"%s","tool_input":{"file_path":"%s/AGENTS.md"}}' "$r" "$r")" RULES_EVOLVE_BIN="$TMP/does-not-exist"
  assert_contains "no session id never dedupes" "$OUT" "AGENTS.md"

  # garbage and empty stdin
  run_hook edit-warn.sh 'garbage' "$D"
  assert_eq "garbage exit" "$CODE" 0
  assert_empty "garbage output" "$OUT"
  run_hook edit-warn.sh '' "$D"
  assert_eq "empty stdin exit" "$CODE" 0

  # odd repo path stays valid JSON
  q="$TMP/ew \"q\" \\ $MODE"
  mkdir -p "$q/.rules-evolve"
  : >"$q/AGENTS.md"
  esc=$(printf '%s' "$q" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')
  run_hook edit-warn.sh "{\"session_id\":\"sessq\",\"cwd\":\"$esc\",\"tool_input\":{\"file_path\":\"$esc/AGENTS.md\"}}" "$D"
  assert_json "odd path json" "$OUT"
  assert_contains "odd path warns" "$OUT" "AGENTS.md"
}

test_wrapper() {
  : >"$TMP/fake.log"
  rm -f "$TMP/fake.log"
  out=$(RULES_EVOLVE_BIN="$FAKE" FAKE_LOG="$TMP/fake.log" "$SCRIPTS/re.sh" evolve --generations 3 --provider mock --json 2>&1)
  assert_eq "wrapper mock exit" "$?" 0
  assert_contains "wrapper passes args" "$(cat "$TMP/fake.log")" "evolve --generations 3 --provider mock --json"
  out=$(RULES_EVOLVE_BIN="$FAKE" FAKE_LOG="$TMP/fake.log" "$SCRIPTS/re.sh" evolve --provider=replay --replay-file x.jsonl 2>&1)
  assert_eq "wrapper replay exit" "$?" 0
  out=$(RULES_EVOLVE_BIN="$FAKE" "$SCRIPTS/re.sh" evolve --generations 1 --provider openai 2>&1)
  assert_eq "wrapper refuses other provider" "$?" 64
  assert_contains "refusal message" "$out" "Only --provider mock or --provider replay"
  out=$(RULES_EVOLVE_BIN="$FAKE" "$SCRIPTS/re.sh" mutate --provider=anthropic 2>&1)
  assert_eq "wrapper refuses --provider=other" "$?" 64
  out=$(RULES_EVOLVE_BIN="$FAKE" "$SCRIPTS/re.sh" run --provider mock --allow-spend 2>&1)
  assert_eq "wrapper refuses --allow-spend" "$?" 64
  assert_contains "allow-spend message" "$out" "never allowed from the plugin"
  out=$(RULES_EVOLVE_BIN="$FAKE" FAKE_LOG="$TMP/fake2.log" "$SCRIPTS/re.sh" propose --hypothesis "a b  c" --file p.md 2>&1)
  assert_contains "wrapper keeps spaces in one argument" "$(cat "$TMP/fake2.log")" "a b  c"
  out=$(RULES_EVOLVE_BIN="$TMP/does-not-exist" "$SCRIPTS/re.sh" status 2>&1)
  assert_eq "wrapper missing binary exit" "$?" 127
  assert_contains "wrapper missing binary help" "$out" "RULES_EVOLVE_BIN"
  out=$(RULES_EVOLVE_BIN="$FAKE" FAKE_EXIT=3 "$SCRIPTS/re.sh" propose --file x --hypothesis y 2>&1)
  assert_eq "wrapper keeps the binary's exit code" "$?" 3

  if [ -n "$REAL" ]; then
    w=$TMP/wrap-real
    demo_repo "$w"
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" status --json)
    assert_eq "real status via wrapper" "$(printf '%s' "$out" | json_get 'j.data.tree.nodes.length')" 9
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" verify-log --json)
    assert_eq "real verify-log via wrapper" "$(printf '%s' "$out" | json_get 'j.data.valid')" true
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" digest)
    assert_contains "real digest via wrapper" "$out" "Rules lineage digest"
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" export --out "$TMP/export.json" --json)
    assert_eq "real export via wrapper" "$(printf '%s' "$out" | json_get 'j.ok')" true
    [ -s "$TMP/export.json" ] && ok || bad "export file written"
    # propose through the wrapper, the same way the propose command does it
    cp "$w/AGENTS.md" "$TMP/p.md"
    printf '\nRun the tests before finishing.\n' >>"$TMP/p.md"
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" propose --file "$TMP/p.md" --hypothesis "tests first" --json)
    assert_eq "real propose via wrapper" "$(printf '%s' "$out" | json_get 'j.data.outcome')" proposed
    out=$(RULES_EVOLVE_BIN="$REAL" "$SCRIPTS/re.sh" -C "$w" evolve --generations 1 --provider openai 2>&1)
    assert_eq "real binary never reached with a paid provider" "$?" 64
  else
    echo "SKIPPED real binary wrapper tests (no built binary)" >&2
  fi
}

MODES="jq nojq"
command -v jq >/dev/null 2>&1 || MODES="nojq"
for MODE in $MODES; do
  test_session_start
  test_session_start_real
  test_edit_warn
done
MODE=wrapper
test_wrapper

printf 'hooks.test.sh: %s passed, %s failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
