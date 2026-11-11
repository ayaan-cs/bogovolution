#!/bin/sh
# Rebuild the demo lineage from the committed replay file (no key, no cost),
# print its digest, and write the export that the viewer ships with.
#
#   examples/demo/export.sh
#
# Writes examples/demo/tree.json and copies it to viewer/src/demo/tree.json.
# Set RULES_EVOLVE to use a different binary.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${RULES_EVOLVE:-$ROOT/target/debug/rules-evolve}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build -q)
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
DEMO_DIR="$WORK" RULES_EVOLVE="$BIN" "$HERE/demo.sh" > /dev/null

"$BIN" -C "$WORK" export --out "$HERE/tree.json"
"$BIN" -C "$WORK" digest > "$HERE/digest.md"
mkdir -p "$ROOT/viewer/src/demo"
cp "$HERE/tree.json" "$ROOT/viewer/src/demo/tree.json"
echo "Wrote $HERE/digest.md and copied the export to viewer/src/demo/tree.json"
