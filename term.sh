#!/usr/bin/env bash
# Open a Konsole window that matches the screenshot pipeline: Unscii 16 at
# 168x71 cells, running roguemap. Falls back to the current terminal if
# Konsole is not installed.
# Usage: ./term.sh [seed] [size]     (COLS/ROWS env override the geometry)
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --quiet
cols="${COLS:-168}"
rows="${ROWS:-71}"
if command -v konsole >/dev/null; then
  exec konsole \
    -p "Font=unscii,12,-1,5,400,0,0,0,0,0,0,0,0,0,0,1" \
    -p "TerminalColumns=$cols" -p "TerminalRows=$rows" \
    -p "ScrollBarPosition=2" \
    --hide-menubar --hide-tabbar \
    -e "$PWD/target/release/roguemap" "${1:-7}" "${2:-32}"
fi
exec ./target/release/roguemap "${1:-7}" "${2:-32}"
