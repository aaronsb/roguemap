#!/usr/bin/env bash
# Build and run roguemap in the current terminal.
# Usage: ./run.sh [seed] [size]
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --quiet
exec ./target/release/roguemap "${1:-7}" "${2:-32}"
