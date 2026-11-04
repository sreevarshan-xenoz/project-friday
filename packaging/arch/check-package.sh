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
  usr/share/zsh/site-functions/_friday usr/share/applications/friday.desktop \
  usr/share/friday/hyprland/friday.lua usr/share/licenses/friday-git/LICENSE-MIT; do
  grep -qx "$f" <<<"$files" && ok "$f" || bad "missing $f"
done

if command -v namcap >/dev/null; then
  echo "namcap"
  # Errors only; warnings (e.g. dependency hints) are informational.
  out=$(namcap "$pkg" 2>&1 || true); echo "$out" | sed 's/^/    /'
  grep -q ' E: ' <<<"$out" && bad "namcap reported errors" || ok "no namcap errors"
fi

echo "install"
pacman -U --noconfirm "$pkg" >/dev/null && ok "pacman -U" || bad "pacman -U failed"
friday --help | grep -q "friday ask" && ok "friday --help" || bad "friday --help"
command -v desktop-file-validate >/dev/null \
  && { desktop-file-validate /usr/share/applications/friday.desktop && ok "launcher entry is valid" || bad "launcher entry invalid"; }
systemd-analyze verify --user /usr/lib/systemd/user/friday-linux-daemon.service 2>/dev/null \
  && ok "user service verifies" || echo "  - systemd-analyze verify unavailable here (no user manager)"

echo "daemon"
tmp=$(mktemp -d); trap 'kill "$dpid" 2>/dev/null || true; rm -rf "$tmp"' EXIT
export XDG_STATE_HOME="$tmp/state" HOME="$tmp" FRIDAY_LISTEN=127.0.0.1:0
friday-linux-daemon >"$tmp/daemon.log" 2>&1 & dpid=$!
state="$XDG_STATE_HOME/friday/state.json"
for _ in $(seq 50); do [[ "$(jq -r .status "$state" 2>/dev/null)" == running ]] && break; sleep 0.2; done
if [[ "$(jq -r .status "$state" 2>/dev/null)" == running ]]; then ok "daemon starts and publishes state.json"
else bad "daemon did not reach running"; sed 's/^/    /' "$tmp/daemon.log" | tail -20; fi
friday-ctl system-info >/dev/null 2>&1 && ok "friday-ctl talks to it" || bad "friday-ctl system-info failed"

echo
(( fails == 0 )) && echo "package ok" || { echo "$fails problem(s)"; exit 1; }
