#!/bin/sh
# Free, local real model run on the 48 task more-itertools set.
#
#   OUT=<folder> WORK=<folder> run-oss48.sh <more-itertools checkout at ceafee8ce96f4bc1eb74c528bc247c91d648a0bb>
#
# Stages, each with a done marker in OUT, so a rerun with the same OUT and WORK
# skips what is finished and picks up the rest (runs already in the log are not
# run again, and recordings are appended to):
#
#   fixture  build the task repo in WORK/measure (24 train, 24 held out tasks)
#   root     measure the root (REPEATS runs, default from config.toml: 5)
#   oracle   measure oracle-rules.md and verify it against the root
#   islice   measure the islice rule from the 7B run of v2.2.0 and verify it
#   evolve   a combined evolve (GENERATIONS, default 1) in WORK/evolve, a copy
#            of the project made right after the root stage, so the root is not
#            run twice. If one generation was cut off after its mutation was
#            logged, the rerun finishes that child with `run` and `verify`.
#
# All 48 stubs are shown to the model in every task prompt (about 9,700
# tokens), so start the model server with a 16384 token context (-c 16384).
# Nothing leaves this machine: the local provider refuses non-localhost URLs.
#
# Environment: OUT and WORK (both required, keep them for reruns),
# GENERATIONS, SEED (default 7), LOCAL_URL, RULES_EVOLVE.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${RULES_EVOLVE:-$ROOT/target/release/rules-evolve}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build --release -q)
fi
. "$HERE/oss48-tasks.sh"
SRC=${1:?usage: OUT=<folder> WORK=<folder> run-oss48.sh <more-itertools checkout>}
OUT=${OUT:?set OUT to a folder that survives a restart}
WORK=${WORK:?set WORK to a folder that survives a restart}
GENERATIONS=${GENERATIONS:-1}
SEED=${SEED:-7}
LOCAL_URL=${LOCAL_URL:-http://127.0.0.1:8080}
mkdir -p "$OUT" "$WORK"
OUT=$(cd "$OUT" && pwd)
WORK=$(cd "$WORK" && pwd)
WM=$WORK/measure
WE=$WORK/evolve
L="--provider local --local-url $LOCAL_URL --isolation bwrap"
ts() { date '+%Y-%m-%d %H:%M:%S %Z'; }
done_() { sync; echo "$(ts)" > "$OUT/$1.done"; sync; echo "stage $1 done $(ts)"; }
is_done() { [ -f "$OUT/$1.done" ]; }
child_of() {
  "$BIN" -C "$1" propose --parent root --file "$2" --hypothesis "$3" --json \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])'
}

if ! is_done fixture; then
  rm -rf "$WM"
  python3 "$HERE/make-oss-fixture.py" "$SRC" "$WM" "$OSS48_TRAIN" \
    --held-out="$OSS48_HELD_OUT" --tasks-out="$OUT/tasks.toml"
  "$BIN" -C "$WM" init --model local-model >/dev/null
  mv "$OUT/tasks.toml" "$WM/.rules-evolve/tasks.toml"
  # Every task prompt is about 10,000 tokens, so 48 tasks need more than the
  # default cap of 200,000 tokens per run.
  sed -i 's/^max_tokens_per_run = 200000$/max_tokens_per_run = 1000000/' "$WM/.rules-evolve/config.toml"
  grep -q '^max_tokens_per_run = 1000000$' "$WM/.rules-evolve/config.toml"
  done_ fixture
fi

if ! is_done root; then
  echo "stage root start $(ts)"
  "$BIN" -C "$WM" run --version root --seed "$SEED" $L \
    --record "$OUT/root-replay.jsonl" --local-transcript "$OUT/root-transcript.jsonl" >> "$OUT/root-run.txt"
  rm -rf "$WE"
  cp -a "$WM" "$WE"
  done_ root
fi

stage() { # name, rules file, hypothesis
  if is_done "$1"; then return 0; fi
  echo "stage $1 start $(ts)"
  if [ ! -s "$OUT/$1.id" ]; then
    child_of "$WM" "$2" "$3" > "$OUT/$1.id.tmp"
    mv "$OUT/$1.id.tmp" "$OUT/$1.id"
  fi
  v=$(cat "$OUT/$1.id")
  "$BIN" -C "$WM" run --version "$v" --seed "$SEED" $L \
    --record "$OUT/$1-replay.jsonl" --local-transcript "$OUT/$1-transcript.jsonl" >> "$OUT/$1-run.txt"
  "$BIN" -C "$WM" verify --child "$v" > "$OUT/$1-verify.txt" || true
  done_ "$1"
}
stage oracle "$HERE/oracle-rules.md" "Hand written control: import everything used"
stage islice "$HERE/replays/qwen2.5-coder-7b/rules-islice-4e7baaad.md" \
  "7B run 2 gen 2 (accepted on the 8 task train set): import islice"

if ! is_done evolve; then
  echo "stage evolve start $(ts)"
  # A mutation without a verdict means the last try was cut off: finish it.
  open=$(python3 - "$WE/.rules-evolve/log.jsonl" <<'PY'
import json, sys
proposed, judged = [], set()
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "mutation_proposed":
        proposed.append(e["payload"]["child"])
    elif e["kind"] == "verdict":
        judged.add(e["payload"]["child"])
print(next((c for c in proposed if c not in judged), ""))
PY
)
  if [ -n "$open" ]; then
    echo "finishing cut off child $open" | tee -a "$OUT/evolve.txt"
    "$BIN" -C "$WE" run --version "$open" --seed "$SEED" $L \
      --record "$OUT/evolve-replay.jsonl" --local-transcript "$OUT/evolve-transcript.jsonl" >> "$OUT/evolve.txt"
    "$BIN" -C "$WE" verify --child "$open" >> "$OUT/evolve.txt" || true
  else
    start=$(date +%s)
    "$BIN" -C "$WE" evolve --generations "$GENERATIONS" --seed "$SEED" $L \
      --record "$OUT/evolve-replay.jsonl" --local-transcript "$OUT/evolve-transcript.jsonl" >> "$OUT/evolve.txt"
    echo "Wall time: $(($(date +%s) - start)) s" >> "$OUT/evolve.txt"
  fi
  done_ evolve
fi
"$BIN" -C "$WM" verify-log
"$BIN" -C "$WE" verify-log
"$BIN" -C "$WE" export --out "$OUT/evolve-export.json" >/dev/null
echo "all stages done $(ts)"
echo "Projects: $WM $WE"
echo "Outputs: $OUT"
