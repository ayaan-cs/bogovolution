#!/bin/sh
# Free, local real model test of graft.
#
#   run-local-test.sh demo                      the demo fixture (12 toy tasks)
#   run-local-test.sh oss <more-itertools dir>  8 train and 8 held out tasks derived
#                                               from more-itertools
#
# Needs a model server already listening on 127.0.0.1 (see README.md here).
# Nothing leaves this machine: the local provider refuses non-localhost URLs.
#
# Environment:
#   GENERATIONS  evolve generations (default 3)
#   SEED         base seed (default 7)
#   OUT          where the replay file, transcript and outputs go (default ./local-test-out)
#   WORK         the throwaway project folder (default: a new temp folder)
#   LOCAL_URL    model server (default http://127.0.0.1:8080)
#   GRAFT path to the binary (default: target/release/graft)
#   OSS_FUNCS    train functions (default: the 8 below)
#   OSS_HELD_OUT held out functions (default: the 8 below). Set it to an empty
#                string for the train only task set of v2.2.0.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${GRAFT:-$ROOT/target/release/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
MODE=${1:-}
GENERATIONS=${GENERATIONS:-3}
SEED=${SEED:-7}
LOCAL_URL=${LOCAL_URL:-http://127.0.0.1:8080}
OUT=${OUT:-$PWD/local-test-out}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd)
# 8 functions picked from a 17 function baseline: 2 the model usually solves,
# 6 it usually fails (mostly missing imports or wrong edge cases).
OSS_FUNCS=${OSS_FUNCS:-take,tail,nth,flatten,partition,unique_justseen,prepend,quantify}
# 8 related functions the mutation prompt never shows: 3 use islice like take
# and nth, 4 need other itertools or collections imports, 1 needs none.
OSS_HELD_OUT=${OSS_HELD_OUT-consume,roundrobin,iter_index,tabulate,ncycles,powerset,unique_everseen,first_true}

case "$MODE" in
  demo)
    cp -R "$ROOT/examples/demo/fixture/." "$WORK/"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    cp "$ROOT/examples/demo/tasks.toml" "$WORK/.graft/"
    ;;
  oss)
    SRC=${2:?usage: run-local-test.sh oss <more-itertools checkout>}
    python3 "$HERE/make-oss-fixture.py" "$SRC" "$WORK" "$OSS_FUNCS" \
      --held-out="$OSS_HELD_OUT" --tasks-out="$OUT/tasks.toml"
    "$BIN" -C "$WORK" init --model local-model >/dev/null
    mv "$OUT/tasks.toml" "$WORK/.graft/tasks.toml"
    ;;
  *)
    echo "usage: run-local-test.sh demo | oss <more-itertools checkout>" >&2
    exit 1
    ;;
esac

# The replay file and transcript live outside the project, so the model never
# reads them as project files.
rm -f "$OUT/$MODE-replay.jsonl" "$OUT/$MODE-transcript.jsonl"
start=$(date +%s)
"$BIN" -C "$WORK" evolve --generations "$GENERATIONS" --seed "$SEED" \
  --provider local --local-url "$LOCAL_URL" --isolation bwrap \
  --record "$OUT/$MODE-replay.jsonl" --local-transcript "$OUT/$MODE-transcript.jsonl" \
  | tee "$OUT/$MODE-evolve.txt"
end=$(date +%s)
echo "Wall time: $((end - start)) s" | tee -a "$OUT/$MODE-evolve.txt"

"$BIN" -C "$WORK" status
"$BIN" -C "$WORK" verify-log
"$BIN" -C "$WORK" digest > "$OUT/$MODE-digest.txt"
"$BIN" -C "$WORK" export --out "$OUT/$MODE-export.json" >/dev/null
echo "Project: $WORK"
echo "Outputs: $OUT"
echo "Replay it for free: graft -C <fresh copy> evolve --generations $GENERATIONS --seed $SEED --provider replay --replay-file $OUT/$MODE-replay.jsonl"
