#!/bin/sh
# Hidden benchmark check: task $1 passes when its answer file says ok.
grep -qx ok "answers/$1.txt"
