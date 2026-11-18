#!/bin/sh
# Run 3 calibration (paid, cheap): root rules on all 48 more-itertools tasks.
# A: thinking off, effort high, cap 4096, 2 repeats.
# B: default adaptive thinking, cap 4096, 1 repeat (repeats edited to 1).
# Purpose: pick the reply settings and a held out set with signal, from root
# results only. Never echoes the key.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude3
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/calib.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
COMMON="--provider anthropic --allow-spend --api-model claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 4096 --api-timeout 120 --isolation bwrap"
if [ ! -f $C/calibA.done ]; then
  log "A start"
  $B -C $C/calibA run --version root --seed 7 $COMMON --api-thinking off --api-effort high \
    --record $C/calibA-replay.jsonl --api-transcript $C/transcripts/calibA.jsonl >> $C/calib.log 2>&1 || { log "A stopped $?"; exit 1; }
  touch $C/calibA.done
fi
if [ ! -f $C/calibB.done ]; then
  log "B start"
  $B -C $C/calibB run --version root --seed 7 $COMMON \
    --record $C/calibB-replay.jsonl --api-transcript $C/transcripts/calibB.jsonl >> $C/calib.log 2>&1 || { log "B stopped $?"; exit 1; }
  touch $C/calibB.done
fi
log "done, cost $(python3 $C/cost.py)"
