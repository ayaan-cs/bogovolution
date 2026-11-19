#!/bin/sh
# mkproj.sh DIR REPEATS: fresh project from examples/honest with run 3 settings.
set -eu
B=${REPO:?set REPO to the repo root}/target/release/rules-evolve
H=${REPO:?set REPO to the repo root}/examples/honest
rm -rf "$1"; cp -a $H/repo "$1"
$B -C "$1" init --model claude-haiku-5-5 >/dev/null
cp $H/tasks.toml "$1/.rules-evolve/tasks.toml"
sed -i "s/^repeats = 5/repeats = $2/; s/^min_paired_wins = 4/min_paired_wins = 3/; s/^held_out_min_paired_wins = 3/held_out_min_paired_wins = 2/; s/^max_tokens_per_run = 200000/max_tokens_per_run = 400000/; s/^isolation = \"auto\"/isolation = \"bwrap\"/; s/^one_shot_agent = false/one_shot_agent = true/; s/^retries_on_refusal = 0/retries_on_refusal = 1/" "$1/.rules-evolve/config.toml"
