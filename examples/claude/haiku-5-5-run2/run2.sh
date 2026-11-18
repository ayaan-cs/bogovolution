#!/bin/sh
# Paid Claude run 2 (approved by Ayaan, target under $0.25, hard stop $0.50).
# Restartable: stages with done markers; evolve uses --total so a rerun never
# goes past 4 children in all (the run 1 rule plus 3 generations).
# Never echoes the key.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude2
P=$C/proj
A="--provider anthropic --allow-spend --api-model claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 2048 --isolation bwrap --record $C/claude-haiku-5-5-run2-replay.jsonl --api-transcript $C/transcript.jsonl"
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/run.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
if [ ! -f $C/root.done ]; then
  log "root start"
  $B -C $P run --version root --seed 7 $A >> $C/run.log 2>&1 || { log "root stopped, exit $?"; exit 1; }
  touch $C/root.done
fi
if [ ! -f $C/oracle.done ]; then
  log "oracle start"
  if [ ! -s $C/oracle.id ]; then
    $B -C $P --json propose --parent root --file $C/oracle-rules.md \
      --hypothesis "Control: the run 1 generation 1 rule (0ea6c448, raise documented errors, import every module), measured on the new held out tasks" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])' > $C/oracle.id.tmp && mv $C/oracle.id.tmp $C/oracle.id
  fi
  V=$(cat $C/oracle.id)
  $B -C $P run --version "$V" --seed 7 $A >> $C/run.log 2>&1 || { log "oracle run stopped, exit $?"; exit 1; }
  $B -C $P verify --child "$V" >> $C/run.log 2>&1
  touch $C/oracle.done
fi
log "evolve start"
$B -C $P evolve --generations 4 --total --seed 7 $A >> $C/run.log 2>&1
rc=$?
log "evolve exit $rc"
exit $rc
