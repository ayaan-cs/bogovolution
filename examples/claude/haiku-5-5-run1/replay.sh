#!/bin/sh
# Replay Claude run 1 for free: no API key, no network, no model calls.
#
#   replay.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Builds the same 9 task project the paid run used, then runs the recorded
# evolve from replay.jsonl. The checks still run under bwrap, so this needs
# python3 and bwrap. The result must match log.jsonl here: 1 noise, 3 overfit,
# the root stays current. The paid run made 4 generations in two sessions
# (see README.md), so the replay asks for 4.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
BIN=${BOGO:-$ROOT/target/release/bogo}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
SRC=${1:?usage: replay.sh <more-itertools checkout>}
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
TASKS=$(mktemp)
python3 "$ROOT/examples/local-model/make-oss-fixture.py" "$SRC" "$WORK" \
  take,nth,distribute,nth_combination,sort_together,polynomial_from_roots \
  --held-out=ncycles,tabulate,chunked_even --short-context --tasks-out="$TASKS"
"$BIN" -C "$WORK" init --model claude-haiku-5-5 >/dev/null
mv "$TASKS" "$WORK/.bogo/tasks.toml"
cp "$HERE/config.toml" "$WORK/.bogo/config.toml"
"$BIN" -C "$WORK" evolve --generations 4 --seed 7 --provider replay \
  --replay-file "$HERE/replay.jsonl" --isolation bwrap
echo "Project: $WORK"
