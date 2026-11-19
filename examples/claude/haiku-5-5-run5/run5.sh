#!/bin/sh
# Paid Claude run 5 (approved by Ayaan, pt48u): honest example, Haiku 5.5,
# adaptive thinking, cap 8192. Root (3 runs) then evolve to a total of 2
# children. Guard $WORKDIR/claude5/guard.sh kills at $1.90 cumulative.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude5
P=$C/proj
A="--provider anthropic --allow-spend --api-model claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 8192 --api-timeout 120 --isolation bwrap --record $C/claude-haiku-5-5-run5-replay.jsonl --api-transcript $C/transcripts/run5.jsonl"
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/run.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
pgrep -f "claude5/guard.sh" >/dev/null || { log "guard not running"; exit 1; }
python3 -c "import sys; sys.exit(0 if float(open('$C/guard.now').read().split()[1]) < 1.50 else 1)" || { log "cost too high"; exit 1; }
if [ ! -f $C/root.done ]; then
  log "root start"
  $B -C $P run --version root --seed 7 $A >> $C/run.log 2>&1 || { log "root stopped $?"; exit 1; }
  touch $C/root.done
fi
log "evolve start"
$B -C $P evolve --generations 2 --total --seed 7 $A >> $C/run.log 2>&1
log "evolve exit $?, cumulative $(python3 $C/cost.py)"
