#!/bin/sh
# Build the Windows zip on Linux (free): cross compile with mingw-w64 and pack
# the exe, the viewer and the replay demo.
#   rustup target add x86_64-pc-windows-gnu   (and gcc-mingw-w64-x86-64)
#   packaging/windows/build.sh [OUT_DIR]
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${1:-$ROOT/../rules-evolve-dist/windows}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
NAME=graft-$VERSION-windows-x86_64
cd "$ROOT"
# Keep build machine paths (home folder, cargo registry, this checkout) out of
# the exe's panic messages and debug info.
CARGO_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home/user --remap-path-prefix=$CARGO_DIR=/cargo --remap-path-prefix=$ROOT=/src" \
  cargo build --release --target x86_64-pc-windows-gnu -p graft-rules
(cd viewer && npm run build)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
D="$STAGE/$NAME"
mkdir -p "$D/viewer" "$D/examples/demo"
cp target/x86_64-pc-windows-gnu/release/graft.exe "$D/"
cp viewer/dist/index.html "$D/viewer/index.html"
cp -R examples/demo/fixture "$D/examples/demo/"
cp examples/demo/tasks.toml examples/demo/mock.toml examples/demo/replay.jsonl \
   examples/demo/tree.json examples/demo/digest.md "$D/examples/demo/"
cp "$HERE/WINDOWS-SETUP.md" "$HERE/demo.ps1" LICENSE CHANGELOG.md "$D/"
mkdir -p "$OUT"
rm -f "$OUT/$NAME.zip"
(cd "$STAGE" && python3 -m zipfile -c "$OUT/$NAME.zip" "$NAME")
(cd "$OUT" && sha256sum "$NAME.zip" > "$NAME.zip.sha256")
echo "Wrote $OUT/$NAME.zip"
