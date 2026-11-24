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

# FRIDAY itself (daemon, CLI, user service, desktop hooks) is the friday-git
# package from packaging/arch, the same one an existing Arch install uses.
# It is built into a local [friday] repo that ships in the ISO at
# /opt/friday/repo: mkarchiso installs it on the live system (packages.x86_64)
# and the installer pacstraps it onto the target. makepkg packages the
# committed HEAD. SKIP_PACKAGE_BUILD=true reuses the package already there.
FRIDAY_REPO_DIR="$PROFILE_DIR/airootfs/opt/friday/repo"
if [[ "${SKIP_PACKAGE_BUILD:-false}" != "true" ]]; then
  if [[ $EUID -eq 0 ]]; then
    echo "Error: makepkg refuses to run as root. Run build-iso.sh as a normal user" >&2
    echo "  (it uses sudo for mkarchiso), or build the package first and set" >&2
    echo "  SKIP_PACKAGE_BUILD=true." >&2
    exit 1
  fi
  if [[ -n "$(git -C "$PROJECT_ROOT" status --porcelain 2>/dev/null)" ]]; then
    echo "Note: uncommitted changes are not in the package (makepkg builds HEAD)."
  fi
  echo "Building the FRIDAY package..."
  mkdir -p "$FRIDAY_REPO_DIR"
  find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'friday*' -type f -delete
  # Build from a scratch copy of the recipe: makepkg writes the computed
  # pkgver back into PKGBUILD and leaves src/ and pkg/ next to it, which would
  # dirty the checkout (and block the next pull).
  PKG_BUILD_DIR=$(mktemp -d)
  cp "$PROJECT_ROOT/packaging/arch/PKGBUILD" "$PROJECT_ROOT/packaging/arch/friday.install" "$PKG_BUILD_DIR/"
  (cd "$PKG_BUILD_DIR" && FRIDAY_REPO="file://$PROJECT_ROOT" PKGDEST="$FRIDAY_REPO_DIR" \
    makepkg -f --noconfirm --syncdeps --nocheck)
  rm -rf "$PKG_BUILD_DIR"

  # Pocketdesk (the phone as a screen, keyboard and remote; it can also answer
  # FRIDAY's questions): the pinned release, repackaged, no compile.
  echo "Building the Pocketdesk package..."
  find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'pocketdesk*' -type f -delete
  PD_BUILD_DIR=$(mktemp -d)
  cp "$PROJECT_ROOT/packaging/arch/pocketdesk/PKGBUILD" "$PROJECT_ROOT/packaging/arch/pocketdesk/pocketdesk.install" "$PD_BUILD_DIR/"
  (cd "$PD_BUILD_DIR" && PKGDEST="$FRIDAY_REPO_DIR" makepkg -f --noconfirm --syncdeps --nocheck)
  rm -rf "$PD_BUILD_DIR"

  # FRIDAY's wallpapers: the pinned set, downloaded and packaged, no build.
  echo "Building the wallpapers package..."
  find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'friday-wallpapers*' -type f -delete
  WP_BUILD_DIR=$(mktemp -d)
  cp "$PROJECT_ROOT/packaging/arch/wallpapers/PKGBUILD" "$WP_BUILD_DIR/"
  (cd "$WP_BUILD_DIR" && PKGDEST="$FRIDAY_REPO_DIR" makepkg -f --noconfirm --nocheck)
  rm -rf "$WP_BUILD_DIR"
fi
FRIDAY_PKG=$(find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'friday-git-*.pkg.tar.zst' 2>/dev/null | sort | tail -n1)
if [[ -z "$FRIDAY_PKG" ]]; then
  echo "Error: no friday-git package in $FRIDAY_REPO_DIR" >&2
  echo "  Run without SKIP_PACKAGE_BUILD=true (needs base-devel and rust)." >&2
  exit 1
fi
POCKETDESK_PKG=$(find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'pocketdesk-*.pkg.tar.zst' 2>/dev/null | sort | tail -n1)
if [[ -z "$POCKETDESK_PKG" ]]; then
  echo "Error: no pocketdesk package in $FRIDAY_REPO_DIR" >&2
  echo "  Run without SKIP_PACKAGE_BUILD=true." >&2
  exit 1
fi
WALLPAPERS_PKG=$(find "$FRIDAY_REPO_DIR" -maxdepth 1 -name 'friday-wallpapers-*.pkg.tar.zst' 2>/dev/null | sort | tail -n1)
if [[ -z "$WALLPAPERS_PKG" ]]; then
  echo "Error: no friday-wallpapers package in $FRIDAY_REPO_DIR" >&2
  echo "  Run without SKIP_PACKAGE_BUILD=true." >&2
  exit 1
fi
rm -f "$FRIDAY_REPO_DIR"/friday.db* "$FRIDAY_REPO_DIR"/friday.files*
repo-add -q "$FRIDAY_REPO_DIR/friday.db.tar.gz" "$FRIDAY_PKG" "$POCKETDESK_PKG" "$WALLPAPERS_PKG"
# The profile's pacman.conf plus the local repo, for this build only.
BUILD_PACMAN_CONF=$(mktemp)
trap 'rm -f "$BUILD_PACMAN_CONF"' EXIT
{
  cat "$PROFILE_DIR/pacman.conf"
  printf '\n[friday]\nSigLevel = Optional TrustAll\nServer = file://%s\n' "$FRIDAY_REPO_DIR"
} >"$BUILD_PACMAN_CONF"

# Stage the pinned Omarchy checkout (gitignored; fetched at a fixed commit,
# never "latest"). Keeps an existing hand-staged copy; see fetch-omarchy.sh.
bash "$SCRIPT_DIR/fetch-omarchy.sh"

# Ensure Omarchy and Friday plugins are fully synced
echo "Synchronizing Omarchy shell and Friday OS desktop plugins..."
mkdir -p "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.shell" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.context" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.bar" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"
cp -r "$PROJECT_ROOT/friday-os/plugins/friday.lock" "$PROFILE_DIR/airootfs/usr/share/omarchy/shell/plugins/"

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
sudo mkarchiso -v -C "$BUILD_PACMAN_CONF" -w "$WORK_DIR" -o "$OUT_DIR" "$PROFILE_DIR"

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
