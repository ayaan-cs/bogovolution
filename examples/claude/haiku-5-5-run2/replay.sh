#!/bin/sh
# Replay Claude run 2 for free: no API key, no network, no model calls.
#
#   replay.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Same steps as the paid run: build the 16 task project, run the root, add the
# run 1 generation 1 rule as a control child and judge it, then evolve to a
# total of 4 children. The checks still run under bwrap, so this needs python3
# and bwrap. The log must match log.jsonl here (only the provider differs).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
BIN=${GRAFT:-$ROOT/target/release/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
SRC=${1:?usage: replay.sh <more-itertools checkout>}
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
TASKS=$(mktemp)
python3 "$ROOT/examples/local-model/make-oss-fixture.py" "$SRC" "$WORK" \
  take,nth,distribute,nth_combination,sort_together,polynomial_from_roots \
  --held-out=ncycles,consume,windowed,intersperse,sliding_window,locate,zip_offset,grouper,transpose,circular_shifts \
  --short-context --tasks-out="$TASKS"
"$BIN" -C "$WORK" init --model claude-haiku-5-5 >/dev/null
mv "$TASKS" "$WORK/.graft/tasks.toml"
cp "$HERE/config.toml" "$WORK/.graft/config.toml"
R="--provider replay --replay-file $HERE/replay.jsonl --isolation bwrap"
"$BIN" -C "$WORK" run --version root --seed 7 $R
V=$("$BIN" -C "$WORK" --json propose --parent root --file "$HERE/rules/0ea6c448.md" \
  --hypothesis "Control: the run 1 generation 1 rule (0ea6c448, raise documented errors, import every module), measured on the new held out tasks" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])')
"$BIN" -C "$WORK" run --version "$V" --seed 7 $R
"$BIN" -C "$WORK" verify --child "$V" || true
"$BIN" -C "$WORK" evolve --generations 4 --total --seed 7 $R
echo "Project: $WORK"
