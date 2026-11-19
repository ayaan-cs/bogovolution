#!/bin/sh
# Replay Claude run 3 for free: no API key, no network, no model calls.
#
#   replay.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Same steps as the paid run (run3.sh): build the 28 task project, run the
# root, add three control children (run 1 rule, run 2 rule, hand written rule)
# and judge each, then evolve to a total of 8 children. The checks still run
# under bwrap, so this needs python3 and bwrap. The log must match log.jsonl
# here (only the provider and timestamps differ).
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
"$BIN" -C "$WORK" init --model claude-haiku-5-5 >/dev/null
mv "$TASKS" "$WORK/.bogo/tasks.toml"
cp "$HERE/config.toml" "$WORK/.bogo/config.toml"
R="--provider replay --replay-file $HERE/replay.jsonl --isolation bwrap"
"$BIN" -C "$WORK" run --version root --seed 7 $R
control() { # file hypothesis
  V=$("$BIN" -C "$WORK" --json propose --parent root --file "$1" --hypothesis "$2" \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])')
  "$BIN" -C "$WORK" run --version "$V" --seed 7 $R
  "$BIN" -C "$WORK" verify --child "$V" || true
}
control "$HERE/rules/0ea6c448.md" "Control: the run 1 generation 1 rule (0ea6c448: raise documented errors, import every module)"
control "$HERE/rules/b2e845f1.md" "Control: the run 2 best held out rule (b2e845f1: every name imported or defined)"
control "$HERE/rules/1ef4720c.md" "Control: hand written general rule (complete file and imports, match examples, itertools style argument checks at call time, strict lengths)"
"$BIN" -C "$WORK" evolve --generations 8 --total --seed 7 $R
echo "Project: $WORK"
