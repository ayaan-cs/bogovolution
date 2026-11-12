#!/bin/sh
# Replay a local model run you recorded yourself: no model server, no network.
#
#   replay.sh demo <demo-replay.jsonl>
#   replay.sh oss <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb> <oss-replay.jsonl>
#   replay.sh oss <checkout> <oss-replay.jsonl> <oss-oracle-replay.jsonl>
#
# The checks still run (under bwrap), so the oss replay needs python3 and bwrap.
# This script does not ship recorded model answers.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${RULES_EVOLVE:-$ROOT/target/release/rules-evolve}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
case "${1:-}" in
  demo)
    REPLAY=${2:?usage: replay.sh demo <demo-replay.jsonl>}
    cp -R "$ROOT/examples/demo/fixture/." "$WORK/"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    cp "$ROOT/examples/demo/tasks.toml" "$WORK/.rules-evolve/"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    ;;
  oss)
    SRC=${2:?usage: replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]}
    REPLAY=${3:?usage: replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]}
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      take,tail,nth,flatten,partition,unique_justseen,prepend,quantify --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.rules-evolve/tasks.toml"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    if [ -n "${4:-}" ]; then
      child=$("$BIN" -C "$WORK" propose --parent root --file "$HERE/oracle-rules.md" --json \
        --hypothesis "Hand written (not from the model): most failures in the baseline were missing imports, so say to import everything used." \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])')
      "$BIN" -C "$WORK" run --version "$child" --seed 7 --provider replay \
        --replay-file "$4"
      "$BIN" -C "$WORK" verify --child "$child"
    fi
    ;;
  *)
    echo "usage: replay.sh demo <demo-replay.jsonl>" >&2
    echo "       replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]" >&2
    exit 1
    ;;
esac
"$BIN" -C "$WORK" verify-log
echo "Project: $WORK"
