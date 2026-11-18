#!/bin/sh
# Paid Claude run 4 (Sonnet allowed by Ayaan only if Haiku plateaus; same
# $2.00 total budget, guard $WORKDIR/claude3/guard.sh kills at $1.90).
# claude-sonnet-5-5 ($2/$10 per MTok), thinking between_tools (its "off"), effort high, cap 3072,
# 1 repeat (max_tokens_per_run 60000 bounds one run to about $0.6).
# Root and the run 3 accepted rule (1ef4720c). Never echoes the key.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude4
P=$C/proj
A="--provider anthropic --allow-spend --api-model claude-sonnet-5-5 --api-no-temperature --api-max-new-tokens 3072 --api-thinking between_tools --api-effort high --api-timeout 120 --isolation bwrap --record $C/claude-sonnet-5-5-run4-replay.jsonl --api-transcript $C/transcripts/run4.jsonl"
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/run.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
pgrep -f "guard.sh" >/dev/null || { log "guard not running"; exit 1; }
under() { python3 -c "import sys; sys.exit(0 if float(open('$WORKDIR/claude3/guard.now').read().split()[1]) < $1 else 1)"; }
under 0.75 || { log "cost too high to start root (worst case of a run is about 0.6)"; exit 1; }
if [ ! -f $C/root.done ]; then
  log "root start"
  $B -C $P run --version root --seed 7 $A >> $C/run.log 2>&1 || { log "root stopped, exit $?"; exit 1; }
  touch $C/root.done
fi
under 1.25 || { log "cost too high to start the rule run"; exit 1; }
if [ ! -f $C/oracle.done ]; then
  log "oracle start"
  if [ ! -s $C/oracle.id ]; then
    $B -C $P --json propose --parent root --file $WORKDIR/claude3/oracle-rules.md \
      --hypothesis "Run 3 accepted rule 1ef4720c (hand written general rule), measured with Sonnet" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])' > $C/oracle.id.tmp && mv $C/oracle.id.tmp $C/oracle.id
  fi
  V=$(cat $C/oracle.id)
  $B -C $P run --version "$V" --seed 7 $A >> $C/run.log 2>&1 || { log "oracle run stopped, exit $?"; exit 1; }
  $B -C $P verify --child "$V" >> $C/run.log 2>&1
  touch $C/oracle.done
fi
log "done, cumulative cost $(python3 $WORKDIR/claude3/cost.py)"
