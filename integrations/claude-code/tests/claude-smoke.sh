#!/bin/sh
# Smoke test with a real Claude Code install, if there is one. No model is called:
#   - `claude plugin validate`, `plugin marketplace add`, `plugin install` only read and write local files
#   - `claude --init-only` runs the SessionStart hooks and exits without a conversation
# Everything happens under a throwaway HOME, so your real ~/.claude is not touched.
# Needs the claude CLI (CLAUDE_BIN or on PATH) and a built bogo binary.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
REPO=$(cd "$ROOT/../.." && pwd)

CLAUDE=${CLAUDE_BIN:-$(command -v claude 2>/dev/null || true)}
if [ -z "$CLAUDE" ] || [ ! -x "$CLAUDE" ]; then
  echo "SKIPPED: claude CLI not installed"
  exit 0
fi
BIN=${BOGO_BIN:-}
if [ -z "$BIN" ]; then
  for c in "$REPO/target/release/bogo" "$REPO/target/debug/bogo"; do
    [ -x "$c" ] && BIN=$c && break
  done
fi
if [ -z "$BIN" ]; then
  echo "SKIPPED: no built bogo binary"
  exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM
export HOME="$TMP/home" DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1
mkdir -p "$HOME"
fail=0
check() { if [ "$1" = 0 ]; then echo "ok: $2"; else echo "FAIL: $2"; fail=1; fi; }

"$CLAUDE" plugin validate "$ROOT/plugin" --strict >/dev/null 2>&1
check $? "plugin validate --strict"
"$CLAUDE" plugin validate "$ROOT" >/dev/null 2>&1
check $? "marketplace validate"
"$CLAUDE" plugin marketplace add "$ROOT" >/dev/null 2>&1
check $? "marketplace add (local path)"
"$CLAUDE" plugin install bogo@bogo-local >/dev/null 2>&1
check $? "plugin install"
details=$("$CLAUDE" plugin details bogo 2>&1)
case "$details" in *"Skills (7)"*) check 0 "7 skills loaded" ;; *) check 1 "7 skills loaded" ;; esac
case "$details" in *"Hooks (2)"*) check 0 "2 hooks loaded" ;; *) check 1 "2 hooks loaded" ;; esac

# SessionStart hook in a repo that has .bogo/
W="$TMP/repo"
DEMO_DIR="$W" BOGO="$BIN" sh "$REPO/examples/demo/demo.sh" >/dev/null 2>&1
(cd "$W" && BOGO_BIN="$BIN" "$CLAUDE" --init-only --debug-file "$TMP/with.log" </dev/null >/dev/null 2>&1)
grep -q 'SessionStart:startup .*finished with status 0' "$TMP/with.log"
check $? "SessionStart hook ran and exited 0"
grep -q 'log intact (64 events); versions 9' "$TMP/with.log"
check $? "SessionStart hook showed the status summary"
grep -q 'provided additionalContext' "$TMP/with.log"
check $? "Claude Code accepted the hook output"

# a repo without .bogo/: the hook stays silent
E="$TMP/plain"
mkdir -p "$E"
(cd "$E" && BOGO_BIN="$BIN" "$CLAUDE" --init-only --debug-file "$TMP/without.log" </dev/null >/dev/null 2>&1)
if grep -q 'bogo:' "$TMP/without.log" && grep -q 'provided additionalContext' "$TMP/without.log"; then check 1 "silent without .bogo"; else check 0 "silent without .bogo"; fi

# switched off
(cd "$W" && BOGO_HOOKS=off BOGO_BIN="$BIN" "$CLAUDE" --init-only --debug-file "$TMP/off.log" </dev/null >/dev/null 2>&1)
if grep -q 'provided additionalContext' "$TMP/off.log"; then check 1 "silent with BOGO_HOOKS=off"; else check 0 "silent with BOGO_HOOKS=off"; fi

[ "$fail" = 0 ]
