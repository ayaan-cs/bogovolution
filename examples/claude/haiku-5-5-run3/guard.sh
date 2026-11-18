#!/bin/sh
# Cumulative guard for all paid calls of runs 3+: kills every paid
# rules-evolve process once the summed cost reaches $1.90. Runs until killed.
while :; do
  c=$(python3 $WORKDIR/claude3/cost.py)
  echo "$(date '+%H:%M:%S') $c" > $WORKDIR/claude3/guard.now
  if python3 -c "import sys; sys.exit(0 if float('$c') >= 1.90 else 1)"; then
    echo "$(date) guard: cumulative cost $c reached 1.90, stopping all paid calls" >> $WORKDIR/claude3/guard.log
    pkill -f -- "--provider anthropic"
    pkill -f "$WORKDIR/claude[34]/.*\.sh"
  fi
  sleep 2
done
