#!/bin/sh
# Run 5 calibration (paid, cheap): root rules on the 24 honest tasks, 2 runs.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude5
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/calib.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
pgrep -f "claude5/guard.sh" >/dev/null || { log "guard not running"; exit 1; }
$B -C $C/calib run --version root --seed 7 --provider anthropic --allow-spend --api-model claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 8192 --api-timeout 120 --isolation bwrap --record $C/calib-replay.jsonl --api-transcript $C/transcripts/calib.jsonl >> $C/calib.log 2>&1 && touch $C/calib.done
log "exit, cumulative $(python3 $C/cost.py)"
