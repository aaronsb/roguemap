#!/usr/bin/env bash
# Check every relative link and image in README.md and docs/**/*.md.
# A link resolves when the path exists relative to the file that names it.
# External links (http, https, mailto) and pure anchors are skipped.
# Usage: tools/check-docs.sh
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
checked=0

files=$(find . -name '*.md' -not -path './target/*' -not -path './.git/*' | sort)

for f in $files; do
  dir=$(dirname "$f")
  # Markdown inline links and images: [text](target) and ![alt](target).
  targets=$(grep -oE '\]\([^)]+\)' "$f" | sed -E 's/^\]\(//; s/\)$//' || true)
  for t in $targets; do
    case "$t" in
      http://*|https://*|mailto:*|'#'*) continue ;;
    esac
    path="${t%%#*}"
    [ -z "$path" ] && continue
    checked=$((checked + 1))
    if [ ! -e "$dir/$path" ]; then
      echo "BROKEN $f -> $t"
      fail=1
    fi
  done
done

if [ "$fail" = 0 ]; then
  echo "links: $checked relative links resolve"
else
  echo "links: broken links above"
fi
exit "$fail"
