#!/bin/sh
# Watch the demo lineage grow live in the terminal. No key, no cost.
#
#   examples/demo/watch.sh            replay at 30 ms per model call (about 20 seconds)
#   examples/demo/watch.sh 100        slower: 100 ms per model call
#   examples/demo/watch.sh 0          as fast as it goes
#
# It copies the fixture to a temp folder, runs `init`, and starts
# `graft watch --run "evolve ..."`, which runs the replay as a child
# process while the view follows the log. Press q to leave (twice while the
# replay is still running, which also stops it). Set GRAFT to use a
# different binary and DEMO_DIR to pick the scratch folder (empty or new).
set -eu

PACE=${1:-30}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${GRAFT:-$ROOT/target/debug/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build -q)
fi

WORK=${DEMO_DIR:-$(mktemp -d)}
mkdir -p "$WORK"
cp -R "$HERE/fixture/." "$WORK/"

"$BIN" -C "$WORK" init --model demo-model > /dev/null
cp "$HERE/tasks.toml" "$HERE/mock.toml" "$WORK/.graft/"

"$BIN" -C "$WORK" watch --run \
  "evolve --generations 10 --seed 7 --provider replay --replay-file '$HERE/replay.jsonl' --pace $PACE"

"$BIN" -C "$WORK" verify-log
echo "Demo lineage is in $WORK/.graft"
