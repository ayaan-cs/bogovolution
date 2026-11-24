#!/bin/sh
# Replay a local model run you recorded yourself: no model server, no network.
#
#   replay.sh demo <demo-replay.jsonl>
#   replay.sh oss <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb> <oss-replay.jsonl>
#   replay.sh oss <checkout> <oss-replay.jsonl> <oss-oracle-replay.jsonl>
#   replay.sh oss-held-out <checkout> <oss-held-out-replay.jsonl>
#   replay.sh held-out-eval <checkout> <held-out-eval-replay.jsonl>
#   replay.sh oss48 <checkout> <folder with root-, oracle-, islice- and evolve-replay.jsonl>
#
# held-out-eval runs the 8 held out functions alone against the root, the two
# rules the 7B model wrote in the v2.2.0 run, and oracle-rules.md.
#
# oss uses the 8 train tasks of v2.2.0. oss-held-out adds the 8 held out tasks
# of v2.3.0 (the run-local-test.sh default), so the held out gate applies.
#
# oss48 replays run-oss48.sh: the 48 task set (oss48-tasks.sh), the root, the
# oracle and the islice rule measured, then a combined evolve from a copy that
# already holds the root runs. GENERATIONS and LATER_REPEATS must match the
# recorded run (the committed oss48 recording: GENERATIONS=1 LATER_REPEATS=2).
# SHORT_CONTEXT=1 replays a run recorded with short prompts (the oss48-short
# folder: SHORT_CONTEXT=1 GENERATIONS=2 NEXT_REPLAY=evolve-gen3-replay.jsonl,
# default 5 repeats, no islice stage). Its first generation 3 lost the model
# server, so the calls it never made are not in evolve-replay.jsonl and
# GENERATIONS=3 stops there with a replay miss; the rerun is NEXT_REPLAY.
#
# The checks still run (under bwrap), so the oss replay needs python3 and bwrap.
# This script does not ship recorded model answers.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${GRAFT:-$ROOT/target/release/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
# The binary reads --replay-file relative to the project (-C), so make the
# replay paths absolute here.
abs() { case "$1" in /*) printf '%s' "$1" ;; *) printf '%s/%s' "$PWD" "$1" ;; esac; }
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
case "${1:-}" in
  demo)
    REPLAY=$(abs "${2:?usage: replay.sh demo <demo-replay.jsonl>}")
    cp -R "$ROOT/examples/demo/fixture/." "$WORK/"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    cp "$ROOT/examples/demo/tasks.toml" "$WORK/.graft/"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    ;;
  oss)
    SRC=${2:?usage: replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]}
    REPLAY=$(abs "${3:?usage: replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]}")
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      take,tail,nth,flatten,partition,unique_justseen,prepend,quantify --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.graft/tasks.toml"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    if [ -n "${4:-}" ]; then
      child=$("$BIN" -C "$WORK" propose --parent root --file "$HERE/oracle-rules.md" --json \
        --hypothesis "Hand written (not from the model): most failures in the baseline were missing imports, so say to import everything used." \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])')
      "$BIN" -C "$WORK" run --version "$child" --seed 7 --provider replay \
        --replay-file "$(abs "$4")"
      "$BIN" -C "$WORK" verify --child "$child"
    fi
    ;;
  oss-held-out)
    SRC=${2:?usage: replay.sh oss-held-out <more-itertools checkout> <replay.jsonl>}
    REPLAY=$(abs "${3:?usage: replay.sh oss-held-out <more-itertools checkout> <replay.jsonl>}")
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      take,tail,nth,flatten,partition,unique_justseen,prepend,quantify \
      --held-out=consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true \
      --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.graft/tasks.toml"
    "$BIN" -C "$WORK" evolve --generations 3 --seed 7 --provider replay \
      --replay-file "$REPLAY"
    ;;
  held-out-eval)
    SRC=${2:?usage: replay.sh held-out-eval <more-itertools checkout> <replay.jsonl>}
    REPLAY=$(abs "${3:?usage: replay.sh held-out-eval <more-itertools checkout> <replay.jsonl>}")
    R7=$HERE/replays/qwen2.5-coder-7b
    TASKS=$(mktemp)
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" \
      consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true \
      --tasks-out="$TASKS"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$TASKS" "$WORK/.graft/tasks.toml"
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
  oss48)
    SRC=${2:?usage: replay.sh oss48 <more-itertools checkout> <replay folder>}
    RD=$(abs "${3:?usage: replay.sh oss48 <more-itertools checkout> <replay folder>}")
    . "$HERE/oss48-tasks.sh"
    WM=$WORK/measure
    TASKS=$(mktemp)
    SC=""
    [ -n "${SHORT_CONTEXT:-}" ] && SC=--short-context
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WM" "$OSS48_TRAIN" \
      --held-out="$OSS48_HELD_OUT" --tasks-out="$TASKS" $SC
    "$BIN" -C "$WM" init --model local-model >/dev/null
    mv "$TASKS" "$WM/.graft/tasks.toml"
    # Every task prompt is about 10,000 tokens, so 48 tasks need more than the
    # default cap of 200,000 tokens per run.
    sed -i 's/^max_tokens_per_run = 200000$/max_tokens_per_run = 1000000/' "$WM/.graft/config.toml"
    "$BIN" -C "$WM" run --version root --seed 7 --provider replay --replay-file "$RD/root-replay.jsonl"
    cp -a "$WM" "$WORK/evolve"
    if [ -n "${LATER_REPEATS:-}" ]; then
      for p in "$WM" "$WORK/evolve"; do
        sed -i -e "s/^repeats = .*/repeats = $LATER_REPEATS/" \
          -e "s/^min_paired_wins = .*/min_paired_wins = $LATER_REPEATS/" \
          -e "s/^held_out_min_paired_wins = .*/held_out_min_paired_wins = $LATER_REPEATS/" \
          "$p/.graft/config.toml"
      done
    fi
    prop() {
      "$BIN" -C "$WM" propose --parent root --file "$1" --hypothesis "$2" --json \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])'
    }
    O=$(prop "$HERE/oracle-rules.md" "Hand written control: import everything used")
    "$BIN" -C "$WM" run --version "$O" --seed 7 --provider replay --replay-file "$RD/oracle-replay.jsonl"
    "$BIN" -C "$WM" verify --child "$O" || true
    # The islice stage was deferred in the short prompt round, so its folder
    # has no islice recording.
    if [ -f "$RD/islice-replay.jsonl" ]; then
      I=$(prop "$HERE/replays/qwen2.5-coder-7b/rules-islice-4e7baaad.md" "7B run 2 gen 2 (accepted on the 8 task train set): import islice")
      "$BIN" -C "$WM" run --version "$I" --seed 7 --provider replay --replay-file "$RD/islice-replay.jsonl"
      "$BIN" -C "$WM" verify --child "$I" || true
    fi
    "$BIN" -C "$WM" verify-log
    "$BIN" -C "$WORK/evolve" evolve --generations "${GENERATIONS:-1}" --seed 7 --provider replay \
      --replay-file "$RD/evolve-replay.jsonl"
    # NEXT_REPLAY names a recording, in the same folder, of one more
    # generation run later on top of the first GENERATIONS (oss48-short:
    # GENERATIONS=2 NEXT_REPLAY=evolve-gen3-replay.jsonl, the rerun of a
    # generation 3 whose model server died).
    if [ -n "${NEXT_REPLAY:-}" ]; then
      "$BIN" -C "$WORK/evolve" evolve --generations 1 --seed 7 --provider replay \
        --replay-file "$RD/$NEXT_REPLAY"
    fi
    WORK=$WORK/evolve
    ;;
  *)
    echo "usage: replay.sh demo <demo-replay.jsonl>" >&2
    echo "       replay.sh oss <more-itertools checkout> <oss-replay.jsonl> [oracle-replay.jsonl]" >&2
    echo "       replay.sh oss-held-out <more-itertools checkout> <oss-held-out-replay.jsonl>" >&2
    echo "       replay.sh held-out-eval <more-itertools checkout> <held-out-eval-replay.jsonl>" >&2
    echo "       replay.sh oss48 <more-itertools checkout> <replay folder>" >&2
    exit 1
    ;;
esac
"$BIN" -C "$WORK" verify-log
echo "Project: $WORK"
