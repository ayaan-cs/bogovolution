#!/bin/sh
# Run every plugin test. Free: mock and replay only, no network, no model.
#   integrations/claude-code/tests/run-all.sh
# If the `claude` CLI is on PATH (or CLAUDE_BIN is set) it also runs
# `claude plugin validate` on the plugin and the marketplace. That command only
# reads files. Without the CLI that step is skipped and the structural tests above
# are the only check.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
status=0

echo "== structural tests (node --test)"
node --test "$HERE"/*.test.mjs || status=1

echo "== hook and wrapper tests"
sh "$HERE/hooks.test.sh" || status=1

CLAUDE=${CLAUDE_BIN:-$(command -v claude 2>/dev/null || true)}
if [ -n "$CLAUDE" ] && [ -x "$CLAUDE" ]; then
  echo "== claude plugin validate"
  "$CLAUDE" plugin validate "$ROOT/plugin" || status=1
  "$CLAUDE" plugin validate "$ROOT" || status=1
  echo "== claude smoke (no model call, throwaway HOME)"
  CLAUDE_BIN="$CLAUDE" sh "$HERE/claude-smoke.sh" || status=1
else
  echo "== claude plugin validate: SKIPPED (claude CLI not installed). Run it yourself:"
  echo "   claude plugin validate $ROOT/plugin && claude plugin validate $ROOT"
fi
exit $status
