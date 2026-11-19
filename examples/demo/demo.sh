#!/bin/sh
# Run the demo lineage in a scratch copy of the fixture repo. No key, no cost.
#
#   examples/demo/demo.sh            replay the committed answers (default)
#   examples/demo/demo.sh --record   run the mock model and rewrite replay.jsonl
#
# Set BOGO to use a different binary. Set DEMO_DIR to pick the scratch
# folder (it must be empty or new); otherwise a temp folder is used.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${BOGO:-$ROOT/target/debug/bogo}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build -q)
fi

WORK=${DEMO_DIR:-$(mktemp -d)}
mkdir -p "$WORK"
cp -R "$HERE/fixture/." "$WORK/"

"$BIN" -C "$WORK" init --model demo-model
cp "$HERE/tasks.toml" "$HERE/mock.toml" "$WORK/.bogo/"

if [ "${1:-}" = "--record" ]; then
  rm -f "$HERE/replay.jsonl"
  "$BIN" -C "$WORK" evolve --generations 10 --seed 7 --provider mock --record "$HERE/replay.jsonl"
else
  "$BIN" -C "$WORK" evolve --generations 10 --seed 7 --provider replay --replay-file "$HERE/replay.jsonl"
fi

"$BIN" -C "$WORK" status
"$BIN" -C "$WORK" verify-log
echo "Demo lineage is in $WORK/.bogo"
