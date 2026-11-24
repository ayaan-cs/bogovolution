#!/bin/sh
# Replay Claude run 5 (the honest example) for free: no key, no network.
#
#   examples/claude/haiku-5-5-run5/replay.sh
#
# Builds a fresh project from examples/honest, runs the root and evolves to a
# total of 2 children, answering only from replay.jsonl. The checks still run
# under bwrap, so this needs python3 and bwrap. The log must match log.jsonl
# here (only the provider and timestamps differ).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
BIN=${GRAFT:-$ROOT/target/release/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
WORK=${WORK:-$(mktemp -d)}
rm -rf "$WORK"
cp -a "$ROOT/examples/honest/repo" "$WORK"
"$BIN" -C "$WORK" init --model claude-haiku-5-5 >/dev/null
cp "$ROOT/examples/honest/tasks.toml" "$WORK/.graft/tasks.toml"
cp "$HERE/config.toml" "$WORK/.graft/config.toml"
R="--provider replay --replay-file $HERE/replay.jsonl --isolation bwrap"
"$BIN" -C "$WORK" run --version root --seed 7 $R
"$BIN" -C "$WORK" evolve --generations 2 --total --seed 7 $R
echo "Project: $WORK"
