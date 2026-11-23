#!/usr/bin/env bash
# store-zip.sh <out.zip>: FRIDAY's browser extension packed for a store (Edge
# Add-ons). Stores give the extension their own id and refuse the manifest's
# "key" (which pins the id for the copy FRIDAY loads itself), so it's left out;
# so is background.scripts, which only Firefox reads and Edge refuses.
set -euo pipefail
out=$(realpath -m "${1:?usage: store-zip.sh <out.zip>}")
src=$(dirname "$(realpath "$0")")/extension
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp -r "$src/." "$tmp/"
jq 'del(.key, .background.scripts)' "$src/manifest.json" >"$tmp/manifest.json"
rm -f "$out"
(cd "$tmp" && python3 -m zipfile -c "$out" ./*)
echo "$out"
