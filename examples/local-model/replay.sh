#!/bin/sh
# Replay a local model run you recorded yourself: no model server, no network.
#
#   replay.sh demo <demo-replay.jsonl>
#   replay.sh oss <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb> <oss-replay.jsonl>
#   replay.sh oss <checkout> <oss-replay.jsonl> <oss-oracle-replay.jsonl>
#   replay.sh oss-held-out <checkout> <oss-held-out-replay.jsonl>
#   replay.sh held-out-eval <checkout> <held-out-eval-replay.jsonl>
#
# held-out-eval runs the 8 held out functions alone against the root, the two
# rules the 7B model wrote in the v2.2.0 run, and oracle-rules.md.
#
# oss uses the 8 train tasks of v2.2.0. oss-held-out adds the 8 held out tasks
# of v2.3.0 (the run-local-test.sh default), so the held out gate applies.
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
  oss-held-out)
    SRC=${2:?usage: replay.sh oss-held-out <more-itertools checkout> <replay.jsonl>}
    REPLAY=${3:?usage: replay.sh oss-held-out <more-itertools checkout> <replay.jsonl>}
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      take,tail,nth,flatten,partition,unique_justseen,prepend,quantify \
      --held-out=consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true \
      --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.rules-evolve/tasks.toml"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    ;;
  held-out-eval)
    SRC=${2:?usage: replay.sh held-out-eval <more-itertools checkout> <replay.jsonl>}
    REPLAY=${3:?usage: replay.sh held-out-eval <more-itertools checkout> <replay.jsonl>}
    R7=$HERE/replays/qwen2.5-coder-7b
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true \
      --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.rules-evolve/tasks.toml"
    prop() {
      "$BIN" -C "$WORK" propose --parent root --file "$1" --hypothesis "$2" --json \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])'
    }
    G=$(prop "$R7/rules-generic-cfd78154.md" "7B run 2 gen 1 (rejected on train): import necessary modules")
    I=$(prop "$R7/rules-islice-4e7baaad.md" "7B run 2 gen 2 (accepted on train): import islice")
    O=$(prop "$HERE/oracle-rules.md" "Hand written control: import everything used")
    for v in root "$G" "$I" "$O"; do
      "$BIN" -C "$WORK" run --version "$v" --seed 7 --provider replay --replay-file "$REPLAY"
    done
    for v in "$G" "$I" "$O"; do "$BIN" -C "$WORK" verify --child "$v" || true; done
    ;;
  *)
    echo "usage: replay.sh demo <demo-replay.jsonl>" >&2
    echo "       replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]" >&2
    echo "       replay.sh oss-held-out <more-itertools checkout> <oss-held-out-replay.jsonl>" >&2
    echo "       replay.sh held-out-eval <more-itertools checkout> <held-out-eval-replay.jsonl>" >&2
    exit 1
    ;;
esac
"$BIN" -C "$WORK" verify-log
echo "Project: $WORK"
