#!/bin/sh
# Prove the starter template works end to end, for free: no key, no network.
#
#   examples/starter/try.sh
#
# 1. Self check: each reference fix passes its hidden test and each file as
#    shipped fails it (needs python3 3.11 or later; skipped otherwise).
# 2. Copies sample-repo/ to a scratch folder, runs `init`, installs
#    tasks.toml and mock.toml, and sets the settings STARTER.md suggests.
# 3. `evolve --generations 3 --total` with the mock model, recording a
#    replay file. The checks are the real Python tests, run isolated.
# 4. The same command again does nothing: the total of 3 is reached.
# 5. A second fresh copy replays the recording and must end with the same
#    log (apart from the provider name).
#
# Needs sh and python3, plus an isolation backend (bwrap on Linux; podman or
# docker elsewhere, with an image that has python3, see STARTER.md).
# ISOLATION=none runs checks unisolated, which only the mock and replay
# providers allow. BOGO picks the binary, WORK the scratch folder.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${BOGO:-$ROOT/target/debug/bogo}
if [ ! -x "$BIN" ] && [ ! -x "$BIN.exe" ]; then
  (cd "$ROOT" && cargo build -q)
fi
WORK=${WORK:-$(mktemp -d)}
mkdir -p "$WORK"
ISO=${ISOLATION:-auto}
PY=${PYTHON:-python3}

echo "== 1. self check of the sample tasks"
"$PY" "$HERE/selfcheck.py" "$HERE"

setup() { # folder
  cp -R "$HERE/sample-repo/." "$1/"
  "$BIN" -C "$1" init --model starter-mock >/dev/null
  cp "$HERE/tasks.toml" "$HERE/mock.toml" "$1/.bogo/"
  # The settings STARTER.md suggests for a first real run.
  sed -i.bak -e 's/^repeats = .*/repeats = 3/' \
    -e 's/^min_paired_wins = .*/min_paired_wins = 3/' \
    -e 's/^held_out_min_paired_wins = .*/held_out_min_paired_wins = 2/' \
    -e 's/^one_shot_agent = .*/one_shot_agent = true/' \
    "$1/.bogo/config.toml"
  rm -f "$1/.bogo/config.toml.bak"
}

echo "== 2. set up $WORK/mock"
mkdir -p "$WORK/mock" "$WORK/replay"
setup "$WORK/mock"

echo "== 3. evolve with the mock model, recording"
"$BIN" -C "$WORK/mock" evolve --generations 3 --total --seed 7 \
  --provider mock --isolation "$ISO" --record "$WORK/starter-replay.jsonl"

echo "== 4. the same command again stops at the total"
"$BIN" -C "$WORK/mock" evolve --generations 3 --total --seed 7 \
  --provider mock --isolation "$ISO" --record "$WORK/starter-replay.jsonl"

echo "== 5. replay in a fresh copy"
setup "$WORK/replay"
"$BIN" -C "$WORK/replay" evolve --generations 3 --total --seed 7 \
  --provider replay --replay-file "$WORK/starter-replay.jsonl" --isolation "$ISO"
"$BIN" -C "$WORK/replay" verify-log
"$PY" - "$WORK/mock/.bogo/log.jsonl" "$WORK/replay/.bogo/log.jsonl" <<'PY'
import json, sys

def events(path):
    out = []
    for line in open(path, encoding="utf-8"):
        e = json.loads(line)
        if e["kind"] == "init":
            continue
        p = e["payload"]
        p.pop("provider", None)
        out.append((e["kind"], json.dumps(p, sort_keys=True)))
    return out

a, b = events(sys.argv[1]), events(sys.argv[2])
runs = [json.loads(p) for k, p in a if k == "run_recorded"]
statuses = {t["status"] for r in runs for t in r["tasks"]}
if a != b:
    sys.exit("FAIL: the replay log differs from the recorded one")
if not {"passed", "failed"} <= statuses:
    sys.exit(f"FAIL: expected both passed and failed tasks, got {sorted(statuses)}")
verdicts = [json.loads(p)["verdict"] for k, p in a if k == "verdict"]
print(f"OK: {len(a)} events match, {len(runs)} runs, verdicts {verdicts}")
PY
"$BIN" -C "$WORK/mock" status
echo "Starter project: $WORK/mock (try: bogo -C $WORK/mock digest)"
