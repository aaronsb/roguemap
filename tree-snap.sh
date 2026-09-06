#!/usr/bin/env bash
# Render one L-system species side-on to a PNG, the way snap.sh renders a
# frame: ./tree-snap.sh out.png "gnarled oak" [cols rows seed season] [key=value ...]
set -euo pipefail
cd "$(dirname "$0")"
out="${1:?usage: tree-snap.sh out.png NAME [cols rows seed season] [key=value ...]}"
name="${2:?usage: tree-snap.sh out.png NAME [cols rows seed season] [key=value ...]}"
shift 2
cells="$(mktemp --suffix=.cells)"
trap 'rm -f "$cells"' EXIT
[ -x target/release/roguemap ] || cargo build --release --quiet
./target/release/roguemap --snap-tree "$name" "$cells" "$@"
python3 tools/cells2png.py "$cells" "$out"
echo "$out"
