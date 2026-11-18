#!/bin/sh
# Paid Claude run 3 (approved by Ayaan Nov 17 11:12 PM CT: $2.00 total for new
# calls, guard kills at $1.90). Haiku 5.5, adaptive thinking, cap 8192.
# Stages with done markers; evolve uses --total so a rerun never goes past
# GEN children in all (3 controls plus evolve generations). Never echoes the key.
set -u
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
C=${WORKDIR:?set WORKDIR to the scratch folder}/claude3
P=$C/proj
GEN=${GEN:-8}
A="--provider anthropic --allow-spend --api-model claude-haiku-5-5 --api-no-temperature --api-max-new-tokens 8192 --api-timeout 120 --isolation bwrap --record $C/claude-haiku-5-5-run3-replay.jsonl --api-transcript $C/transcripts/run3.jsonl"
log() { echo "$(date '+%Y-%m-%d %H:%M:%S %Z') $*" >> $C/run.log; }
[ -n "${ANTHROPIC_API_KEY:-}" ] || { log "key not set"; exit 1; }
python3 -c "import sys; sys.exit(0 if float(open('$C/guard.now').read().split()[1]) < 1.80 else 1)" || { log "cost near the limit, not starting"; exit 1; }
pgrep -f "guard.sh" >/dev/null || { log "guard not running"; exit 1; }
if [ ! -f $C/root.done ]; then
  log "root start"
  $B -C $P run --version root --seed 7 $A >> $C/run.log 2>&1 || { log "root stopped, exit $?"; exit 1; }
  touch $C/root.done
fi
control() { # name file hypothesis
  if [ ! -f $C/$1.done ]; then
    log "$1 start"
    if [ ! -s $C/$1.id ]; then
      $B -C $P --json propose --parent root --file "$2" --hypothesis "$3" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["child"])' > $C/$1.id.tmp && mv $C/$1.id.tmp $C/$1.id
    fi
    V=$(cat $C/$1.id)
    $B -C $P run --version "$V" --seed 7 $A >> $C/run.log 2>&1 || { log "$1 run stopped, exit $?"; exit 1; }
    $B -C $P verify --child "$V" >> $C/run.log 2>&1
    touch $C/$1.done
  fi
}
control ctl_run1 $REPO/examples/claude/haiku-5-5-run1/rules/0ea6c448*.md \
  "Control: the run 1 generation 1 rule (0ea6c448: raise documented errors, import every module)"
control ctl_run2 $REPO/examples/claude/haiku-5-5-run2/rules/b2e845f1*.md \
  "Control: the run 2 best held out rule (b2e845f1: every name imported or defined)"
control ctl_oracle $C/oracle-rules.md \
  "Control: hand written general rule (complete file and imports, match examples, itertools style argument checks at call time, strict lengths)"
log "evolve start (total $GEN)"
$B -C $P evolve --generations $GEN --total --seed 7 $A >> $C/run.log 2>&1
rc=$?
log "evolve exit $rc"
exit $rc
