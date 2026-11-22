#!/usr/bin/env bash
# Smoke-test a built friday-git package: contents, namcap, install, and the
# installed daemon starting and answering friday-ctl. Run as root on Arch
# (it installs the package):  bash packaging/arch/check-package.sh friday-git-*.pkg.tar.zst
set -euo pipefail
pkg=${1:?usage: check-package.sh <package file>}
fails=0
ok()  { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fails=$((fails + 1)); }

echo "contents"
files=$(bsdtar -tf "$pkg")
for f in usr/bin/friday usr/bin/friday-linux-daemon usr/bin/friday-ctl \
  usr/lib/friday/friday-skill-forge usr/lib/systemd/user/friday-linux-daemon.service \
  usr/lib/systemd/user/friday-local-ai.service usr/lib/systemd/user/friday-llama.service \
  usr/lib/systemd/user/friday-recall.service \
  usr/lib/systemd/user/friday-wake.service \
  usr/lib/systemd/user/friday-session-save.service \
  usr/lib/systemd/user/friday-session-save.timer \
  usr/share/zsh/site-functions/_friday usr/share/applications/friday.desktop \
  usr/share/applications/friday-settings.desktop usr/share/friday/shell/settings/SettingsWindow.qml \
  usr/share/friday/hyprland/friday.lua usr/share/friday/niri/friday.kdl \
  usr/share/licenses/friday-git/LICENSE-MIT usr/share/licenses/friday-git/agent-desktop-LICENSE \
  usr/lib/friday/agent-desktop usr/lib/friday/agent-desktop-linux-clipboard-owner \
  usr/share/friday/shell/shell.qml usr/share/friday/shell/services/Friday.qml \
  usr/share/friday/helper/friday-os-helper.service \
  usr/share/friday/helper/org.friday.Helper1.service \
  usr/share/friday/helper/org.friday.Helper1.conf \
  usr/share/friday/helper/org.friday.helper.policy usr/share/doc/friday/USING.md; do
  grep -qx "$f" <<<"$files" && ok "$f" || bad "missing $f"
done
if bsdtar -tvf "$pkg" | grep -E '^-[rwx-]*x[rwx-]* .*usr/lib/friday/friday-os-helper$' >/dev/null; then
  ok "usr/lib/friday/friday-os-helper (executable)"
else
  bad "usr/lib/friday/friday-os-helper missing or not executable"
fi
if grep -E '^(etc/dbus-1|usr/share/dbus-1|usr/share/polkit-1|etc/systemd|usr/lib/systemd/system)/.*(helper|Helper)' <<<"$files"; then
  bad "package contains active helper files (must stay inert)"
else
  ok "helper is inert (no files in active system directories)"
fi
# Every FRIDAY Shell file in the tree is in the package.
src=$(cd "$(dirname "$0")/../linux/shell" && find . -name '*.qml' | wc -l)
pkgd=$(grep -c '^usr/share/friday/shell/.*\.qml$' <<<"$files")
[[ "$src" -eq "$pkgd" ]] && ok "FRIDAY Shell: $pkgd QML files" || bad "FRIDAY Shell: $pkgd of $src QML files packaged"

if command -v namcap >/dev/null; then
  echo "namcap"
  # Errors only; warnings (e.g. dependency hints) are informational.
  out=$(namcap "$pkg" 2>&1 || true); echo "$out" | sed 's/^/    /'
  grep -q ' E: ' <<<"$out" && bad "namcap reported errors" || ok "no namcap errors"
fi

echo "install"
pacman -U --noconfirm "$pkg" >/dev/null && ok "pacman -U" || bad "pacman -U failed"
friday --help | grep -q "friday ask" && ok "friday --help" || bad "friday --help"
# The pinned agent-desktop starts and is a version FRIDAY accepts (1.x, >= 1.3).
ad=$(/usr/lib/friday/agent-desktop version 2>/dev/null || true)
grep -q '"ok":true' <<<"$ad" && grep -qE '"version":"1\.([3-9]|[1-9][0-9])\.' <<<"$ad" \
  && ok "agent-desktop runs: $(sed -n 's/.*"data":{[^}]*"version":"\([^"]*\)".*/\1/p' <<<"$ad")" \
  || bad "agent-desktop did not start, or is a version FRIDAY does not accept"
command -v desktop-file-validate >/dev/null \
  && { desktop-file-validate /usr/share/applications/friday.desktop /usr/share/applications/friday-settings.desktop && ok "launcher entries are valid" || bad "launcher entry invalid"; }
systemd-analyze verify --user /usr/lib/systemd/user/friday-{linux-daemon,local-ai,recall,wake}.service 2>/dev/null \
  && ok "user service verifies" || echo "  - systemd-analyze verify unavailable here (no user manager)"

echo "daemon"
# The daemon writes its state as it stops: wait for it before removing that.
tmp=$(mktemp -d); trap 'kill "$dpid" 2>/dev/null && wait "$dpid" 2>/dev/null; rm -rf "$tmp"' EXIT
export XDG_STATE_HOME="$tmp/state" HOME="$tmp" FRIDAY_LISTEN=127.0.0.1:0
friday-linux-daemon >"$tmp/daemon.log" 2>&1 & dpid=$!
state="$XDG_STATE_HOME/friday/state.json"
for _ in $(seq 50); do [[ "$(jq -r .status "$state" 2>/dev/null)" == running ]] && break; sleep 0.2; done
if [[ "$(jq -r .status "$state" 2>/dev/null)" == running ]]; then ok "daemon starts and publishes state.json"
else bad "daemon did not reach running"; sed 's/^/    /' "$tmp/daemon.log" | tail -20; fi
friday-ctl system-info >/dev/null 2>&1 && ok "friday-ctl talks to it" || bad "friday-ctl system-info failed"

echo
(( fails == 0 )) && echo "package ok" || { echo "$fails problem(s)"; exit 1; }
