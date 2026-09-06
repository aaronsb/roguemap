#!/usr/bin/env bash
# Render one headless frame to a PNG with the same font and cell size the
# documentation screenshots use (Unscii 16, 8x16 pixel cells).
# Usage: ./snap.sh out.png [key=value ...]     (COLS/ROWS env override 168x71)
set -euo pipefail
cd "$(dirname "$0")"
out="${1:?usage: snap.sh out.png [key=value ...]}"
shift
cols="${COLS:-168}"
rows="${ROWS:-71}"
cells="$(mktemp --suffix=.cells)"
trap 'rm -f "$cells"' EXIT
[ -x target/release/roguemap ] || cargo build --release --quiet
./target/release/roguemap --snap "$cols" "$rows" "$cells" "$@"
python3 tools/cells2png.py "$cells" "$out"
echo "$out"
