#!/usr/bin/env bash
# Open a Konsole window that reproduces the canonical look of the snapshot
# renderer (tools/cells2png.py): Unscii 16 at exactly 16 pixels per cell
# height, 8 pixels per cell width, 168x71 cells. The point size is derived
# from the screen DPI so the glyphs land on the same pixel grid. Falls back
# to the current terminal if Konsole is not installed.
# A crash leaves its evidence: stderr is teed to $LOG and the window stays
# open on a non-zero exit, since konsole -e closes it the moment the process
# ends and a panic message would go with it.
# Usage: ./term.sh [seed] [size]     (COLS/ROWS/DPI/LOG env override)
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --quiet
cols="${COLS:-168}"
rows="${ROWS:-71}"
dpi="${DPI:-$(xrdb -query 2>/dev/null | awk -F: '/^Xft.dpi/ {gsub(/ /,"",$2); print $2}')}"
dpi="${dpi:-96}"
# 16 px tall glyphs: points = 16 * 72 / dpi
pt="$(awk -v d="$dpi" 'BEGIN { printf "%.2f", 16 * 72 / d }')"
log="${LOG:-/tmp/roguemap-crash.log}"
: > "$log"
run="RUST_BACKTRACE=full '$PWD/target/release/roguemap' '${1:-7}' '${2:-32}' 2> >(tee -a '$log' >&2)
code=\$?
if [ \$code -ne 0 ]; then
  printf '\\nexit %s — stderr above and in %s\\n' \$code '$log'
  read -r -p 'enter to close'
fi
exit \$code"
if command -v konsole >/dev/null; then
  exec konsole \
    -p "Font=unscii,$pt,-1,5,400,0,0,0,0,0,0,0,0,0,0,1" \
    -p "AntiAliasFonts=false" \
    -p "LineSpacing=0" \
    -p "BoldIntense=false" \
    -p "UseFontLineChararacters=false" \
    -p "TerminalColumns=$cols" -p "TerminalRows=$rows" \
    -p "ScrollBarPosition=2" \
    --hide-menubar --hide-tabbar \
    -e bash -c "$run"
fi
exec bash -c "$run"
