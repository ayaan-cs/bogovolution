#!/bin/sh
# Rebuild the pictures of the finished demo in `watch` (no key, no cost):
#
#   docs/screenshots/watch-demo.txt   plain text, checked by a test
#   docs/screenshots/watch-demo.svg   colored picture
#
# The PNG next to them is made from the SVG with viewer/scripts/svg-to-png.mjs
# (needs an installed Chrome), see the README.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
BIN=${GRAFT:-$ROOT/target/debug/graft}
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && cargo build -q)
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
DEMO_DIR="$WORK" GRAFT="$BIN" "$HERE/demo.sh" > /dev/null

OUT="$ROOT/docs/screenshots"
mkdir -p "$OUT"
"$BIN" -C "$WORK" watch --snapshot --width 130 --height 34 --select current > "$OUT/watch-demo.txt"
"$BIN" -C "$WORK" watch --snapshot --width 130 --height 34 --select current --svg "$OUT/watch-demo.svg" > /dev/null
echo "Wrote $OUT/watch-demo.txt and $OUT/watch-demo.svg"
