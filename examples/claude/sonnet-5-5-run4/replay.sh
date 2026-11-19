#!/bin/sh
# Replay Claude run 4 (claude-sonnet-5-5) for free: no API key, no network.
#
#   replay.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Same 28 tasks as run 3, one run of the root and one of the run 3 accepted
# rule (1ef4720c). verify refuses (min_paired_wins 3 is more than 1 repeat),
# as in the paid run. The log must match log.jsonl here.
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
  take,nth,unique_justseen,grouper,sliding_window,distribute,consecutive_groups,stagger,flatten,exactly_n,nth_combination,polynomial_from_roots \
  --held-out=consume,iter_index,circular_shifts,before_and_after,transpose,intersperse,windowed,chunked,locate,zip_offset,reshape,chunked_even,totient,ncycles,powerset,mark_ends \
  --short-context --tasks-out="$TASKS" >/dev/null
"$BIN" -C "$WORK" init --model claude-sonnet-5-5 >/dev/null
mv "$TASKS" "$WORK/.bogo/tasks.toml"
cp "$HERE/config.toml" "$WORK/.bogo/config.toml"
R="--provider replay --replay-file $HERE/replay.jsonl --isolation bwrap"
"$BIN" -C "$WORK" run --version root --seed 7 $R
V=$("$BIN" -C "$WORK" --json propose --parent root --file "$HERE/rules/1ef4720c.md" \
  --hypothesis "Run 3 accepted rule 1ef4720c (hand written general rule), measured with Sonnet" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])')
"$BIN" -C "$WORK" run --version "$V" --seed 7 $R
"$BIN" -C "$WORK" verify --child "$V" || true
echo "Project: $WORK"
