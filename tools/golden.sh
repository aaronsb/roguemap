#!/usr/bin/env bash
# Render a fixed set of headless frames as cell dumps. Two runs on the same
# code must be byte-identical, so `make golden` before a refactor and
# `make golden-check` after proves the output did not change.
# Usage: tools/golden.sh OUT_DIR
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:?usage: golden.sh OUT_DIR}"
mkdir -p "$out"
B=./target/release/roguemap
shot() { name=$1; shift; "$B" --snap 120 40 "$out/$name.cells" "$@"; }
shot island        zoom=0 t=3 tod=12 player=1
shot rotated       zoom=0 t=3 tod=12 deg=25 player=1
shot filled        fill=1 zoom=1 cx=0 cy=0 t=3 tod=12 player=1
shot closeup       fill=1 zoom=3 cx=0 cy=0 t=3 tod=12 player=1
shot night         fill=1 zoom=2 cx=500 cy=-300 t=3 tod=22 fire=1
shot winter        fill=1 zoom=1 cx=500 cy=-300 t=3 tod=12 season=3 simdays=2
shot clouds        fill=1 zoom=0 cx=0 cy=0 t=3 tod=14 cover=0.4
shot steppe        fill=1 zoom=1 cx=-500 cy=400 t=3 tod=12 precip=0.6 cover=0.9 wind=0.7
shot worldmap      fill=1 worldmap=1 scale=2 cx=0 cy=0
shot settings      popover=1 zoom=0
shot ascii         zoom=1 fill=1 cx=0 cy=0 t=3 tod=12 glyphs=ascii
shot scale         scene=scale zoom=3 t=3 tod=12
shot props         fill=1 zoom=3 cx=0 cy=0 t=3 tod=15
echo "wrote $(ls "$out" | wc -l) frames to $out"
