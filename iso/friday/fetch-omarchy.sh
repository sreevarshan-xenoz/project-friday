#!/usr/bin/env bash
# Stage the pinned Omarchy checkout into the ISO profile.
#
# airootfs/usr/share/omarchy/ is gitignored (Omarchy is a frozen upstream
# reference, never committed here), so a fresh clone has no copy and
# mkarchiso fails. This fetches exactly OMARCHY_REF, verifies it, and copies
# it in. It never follows a branch: moving to a newer Omarchy means changing
# the pin below on purpose.
#
#   OMARCHY_REF=<sha>        override the pin (full 40-char SHA)
#   OMARCHY_FORCE=true       replace an unstamped/hand-made copy too
#   OMARCHY_SKIP_FETCH=true  do nothing
set -euo pipefail

OMARCHY_REPO="${OMARCHY_REPO:-https://github.com/basecamp/omarchy.git}"
OMARCHY_PIN="0b3f1b7ead00ac4bcbaae8bac16bab3f7efbe516"
OMARCHY_REF="${OMARCHY_REF:-$OMARCHY_PIN}"

PROFILE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="${OMARCHY_DEST:-$PROFILE_DIR/airootfs/usr/share/omarchy}"
CACHE="${OMARCHY_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/friday-iso/omarchy}"
STAMP="$DEST/.friday-omarchy-ref"

log() { echo "[omarchy] $*"; }

if [[ "${OMARCHY_SKIP_FETCH:-false}" == "true" ]]; then
  log "OMARCHY_SKIP_FETCH=true; leaving $DEST as is"
  exit 0
fi

if [[ ! "$OMARCHY_REF" =~ ^[0-9a-f]{40}$ ]]; then
  echo "[omarchy] OMARCHY_REF must be a full 40-character commit SHA, got '$OMARCHY_REF'" >&2
  exit 1
fi

if [[ -f "$STAMP" && "$(cat "$STAMP")" == "$OMARCHY_REF" && -x "$DEST/bin/omarchy" ]]; then
  log "already staged at ${OMARCHY_REF:0:8}"
  exit 0
fi

# A copy without our stamp was put there by hand (possibly with local
# edits). Don't overwrite it unless asked.
if [[ -x "$DEST/bin/omarchy" && ! -f "$STAMP" && "${OMARCHY_FORCE:-false}" != "true" ]]; then
  log "found a hand-staged copy at $DEST (no stamp); keeping it."
  log "set OMARCHY_FORCE=true to replace it with the pinned ${OMARCHY_REF:0:8}"
  exit 0
fi

log "fetching $OMARCHY_REPO @ ${OMARCHY_REF:0:8}"
mkdir -p "$CACHE"
if [[ ! -d "$CACHE/.git" ]]; then
  git -C "$CACHE" init -q
  git -C "$CACHE" remote add origin "$OMARCHY_REPO"
fi
git -C "$CACHE" fetch -q --depth 1 origin "$OMARCHY_REF"
git -C "$CACHE" -c advice.detachedHead=false checkout -q --force FETCH_HEAD
git -C "$CACHE" clean -qfdx

got="$(git -C "$CACHE" rev-parse HEAD)"
if [[ "$got" != "$OMARCHY_REF" ]]; then
  echo "[omarchy] fetched $got, expected $OMARCHY_REF; refusing to stage it" >&2
  exit 1
fi

# Replace the tree but keep FRIDAY's own plugins, which build-iso.sh copies
# in from friday-os/plugins/ afterwards anyway.
mkdir -p "$DEST"
find "$DEST" -mindepth 1 -maxdepth 1 ! -name shell -exec rm -rf {} +
if [[ -d "$DEST/shell" ]]; then
  find "$DEST/shell" -mindepth 1 -maxdepth 1 ! -name plugins -exec rm -rf {} +
  find "$DEST/shell/plugins" -mindepth 1 -maxdepth 1 ! -name 'friday.*' -exec rm -rf {} + 2>/dev/null || true
fi
tar -C "$CACHE" --exclude=.git -cf - . | tar -C "$DEST" -xf -
echo "$OMARCHY_REF" > "$STAMP"
log "staged ${OMARCHY_REF:0:8} into $DEST"
