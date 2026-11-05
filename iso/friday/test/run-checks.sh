#!/usr/bin/env bash
# Desktop / installer / CLI checks for the FRIDAY ISO profile.
#
# Each check here exists because the same problem once shipped unnoticed.
# Runs on an Arch system (CI uses the archlinux container). Needs:
#   pacman -S --needed git gum jq libnotify python zsh lua kbd tzdata \
#     hyprland quickshell sway grim foot qt6-5compat qt6-imageformats qt6-multimedia \
#     qt6-multimedia-ffmpeg qt6-virtualkeyboard qt6-quicktimeline qt6-svg \
#     qt6-wayland qt6-positioning kirigami syntax-highlighting \
#     ttf-material-symbols-variable noto-fonts ttf-jetbrains-mono-nerd
#
#   bash iso/friday/test/run-checks.sh            # everything available
#   bash iso/friday/test/run-checks.sh static cli # only some groups
#   SKIP_PACKAGES=1 ...                           # no repo lookups (offline)
#
# Groups: static packages installer cli shell
set -uo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PROFILE_DIR="$(cd "$TEST_DIR/.." && pwd)"
export REPO_ROOT="$(cd "$PROFILE_DIR/../.." && pwd)"
export AIROOTFS="$PROFILE_DIR/airootfs"
export WORK="${CHECK_WORK:-$(mktemp -d /tmp/friday-checks.XXXXXX)}"

groups=("$@")
(( ${#groups[@]} )) || groups=(static packages installer cli shell)

failed=()
for g in "${groups[@]}"; do
  script="$TEST_DIR/checks/$g.sh"
  if [[ ! -f "$script" ]]; then
    echo "unknown check group: $g" >&2
    failed+=("$g")
    continue
  fi
  echo
  echo "━━━ $g ━━━"
  runner=(bash "$script")
  # The shell check needs a session bus for notifications.
  if [[ "$g" == shell && -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && command -v dbus-run-session >/dev/null; then
    runner=(dbus-run-session -- bash "$script")
  fi
  if "${runner[@]}"; then
    echo "━━━ $g: ok"
  else
    echo "━━━ $g: FAILED"
    failed+=("$g")
  fi
done

echo
if (( ${#failed[@]} )); then
  echo "FAILED: ${failed[*]}"
  exit 1
fi
echo "All checks passed (${groups[*]})."
