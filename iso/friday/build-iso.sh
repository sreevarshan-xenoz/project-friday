#!/usr/bin/env bash
# FRIDAY OS / Omarchy ISO Production Builder
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

WORK_DIR="${WORK_DIR:-/var/tmp/friday-iso-work}"
OUT_DIR="${OUT_DIR:-/tmp/friday-iso-out}"
PROFILE_DIR="$SCRIPT_DIR"

echo "============================================================"
echo "          FRIDAY OS / OMARCHY ISO BUILD PIPELINE            "
echo "============================================================"
echo "Project Root: $PROJECT_ROOT"
echo "Profile:      $PROFILE_DIR"
echo "Work Dir:     $WORK_DIR"
echo "Output Dir:   $OUT_DIR"
echo "============================================================"

# Ensure required host build tools
if ! command -v mkarchiso &>/dev/null; then
  echo "Error: mkarchiso not found. Please install 'archiso' package." >&2
  exit 1
fi

# Ensure friday binaries exist in airootfs
for bin in friday friday-ctl friday-linux-daemon; do
  if [[ ! -f "$PROFILE_DIR/airootfs/usr/bin/$bin" ]]; then
    echo "Binary $bin not found in airootfs/usr/bin/. Rebuilding..."
    cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml" \
      --package friday-mobile --bin friday-linux-daemon --bin friday-ctl \
      --package friday-skill-forge --bin friday
    cp -f "$PROJECT_ROOT/target/release/friday"* "$PROFILE_DIR/airootfs/usr/bin/"
    strip "$PROFILE_DIR/airootfs/usr/bin/friday"* 2>/dev/null || true
    chmod 755 "$PROFILE_DIR/airootfs/usr/bin/friday"*
  fi
done

# Ensure Omarchy and Friday plugins are fully synced
echo "Synchronizing Omarchy shell and Friday OS desktop plugins..."
mkdir -p "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.shell" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.context" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"

# Ensure executable permissions on all omarchy binaries
chmod -R +x "$PROFILE_DIR/airootfs/usr/share/omarchy/bin" 2>/dev/null || true
chmod +x "$PROFILE_DIR/airootfs/root/configurator" "$PROFILE_DIR/airootfs/root/installer"

# Prepare clean output directory
mkdir -p "$OUT_DIR"

# Check if work dir has stale locks
if [[ -d "$WORK_DIR" && "${CLEAN_WORK:-false}" == "true" ]]; then
  echo "Cleaning previous work directory..."
  sudo rm -rf "$WORK_DIR"
fi

echo "Invoking mkarchiso (requires sudo)..."
sudo mkarchiso -v -w "$WORK_DIR" -o "$OUT_DIR" "$PROFILE_DIR"

LATEST_ISO=$(ls -t "$OUT_DIR"/*.iso 2>/dev/null | head -n 1 || true)
if [[ -n "$LATEST_ISO" && -f "$LATEST_ISO" ]]; then
  echo "============================================================"
  echo "ISO BUILD SUCCESSFUL!"
  echo "Output: $LATEST_ISO"
  ls -lh "$LATEST_ISO"
  sha256sum "$LATEST_ISO"
  mkdir -p "$PROJECT_ROOT/iso/out"
  ln -sf "$LATEST_ISO" "$PROJECT_ROOT/iso/out/friday-latest.iso"
  echo "Latest symlink: $PROJECT_ROOT/iso/out/friday-latest.iso"
  echo "============================================================"
else
  echo "Error: No ISO image found in $OUT_DIR after build." >&2
  exit 1
fi
