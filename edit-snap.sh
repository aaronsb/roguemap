#!/usr/bin/env bash
# Render one editor screen to a PNG with the same font and cell size the
# documentation screenshots use (Unscii 16, 8x16 pixel cells).
# Usage: ./edit-snap.sh out.png [key=value ...]   (COLS/ROWS env override 168x71)
# Keys: dir table row biome season tod glyphs tier pattern deg pane grid.
set -euo pipefail
cd "$(dirname "$0")"
out="${1:?usage: edit-snap.sh out.png [key=value ...]}"
shift
cols="${COLS:-168}"
rows="${ROWS:-71}"
cells="$(mktemp --suffix=.cells)"
trap 'rm -f "$cells"' EXIT
[ -x target/release/roguemap-edit ] || cargo build --release --quiet
./target/release/roguemap-edit --snap "$cols" "$rows" "$cells" "$@"
python3 tools/cells2png.py "$cells" "$out"
echo "$out"
