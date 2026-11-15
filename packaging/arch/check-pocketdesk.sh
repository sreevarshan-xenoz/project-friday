#!/usr/bin/env bash
# Smoke-test the built pocketdesk package: contents, that nothing is turned on,
# install, and that the installed binaries start and find their libraries.
# Run as root on Arch (it installs the package):
#   bash packaging/arch/check-pocketdesk.sh pocketdesk-*.pkg.tar.zst
set -euo pipefail
pkg=${1:?usage: check-pocketdesk.sh <package file>}
fails=0
ok()  { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fails=$((fails + 1)); }

echo "contents"
files=$(bsdtar -tf "$pkg")
for f in usr/bin/pocketdesk usr/bin/pocketdesk-app usr/bin/pocketdesk-tray \
  usr/lib/pocketdesk/pocketdesk-pam-ask usr/lib/systemd/user/pocketdesk.service \
  usr/share/applications/dev.pocketdesk.Pocketdesk.desktop \
  usr/share/icons/hicolor/scalable/apps/pocketdesk.svg \
  usr/share/licenses/pocketdesk/LICENSE-MIT usr/share/licenses/pocketdesk/LICENSE-APACHE; do
  grep -qx "$f" <<<"$files" && ok "$f" || bad "missing $f"
done
# makepkg compresses man pages, so the file may end in .gz.
grep -qE '^usr/share/man/man1/pocketdesk\.1(\.gz)?$' <<<"$files" && ok "the man page" || bad "missing the man page"
# Installed, not running: nothing may switch the service on, or touch PAM.
if grep -E '^(etc/|usr/lib/systemd/user/.*\.wants/|usr/lib/systemd/user-preset|usr/lib/pam\.d|usr/lib/security)' <<<"$files" >/dev/null; then
  bad "the package enables the service or touches /etc or PAM"
else
  ok "the service is installed but not enabled; /etc and PAM are untouched"
fi
bsdtar -xOf "$pkg" usr/lib/systemd/user/pocketdesk.service | grep -qx 'ExecStart=/usr/bin/pocketdesk start' \
  && ok "the unit starts /usr/bin/pocketdesk" || bad "the unit's ExecStart is not /usr/bin/pocketdesk start"

if command -v namcap >/dev/null; then
  echo "namcap"
  out=$(namcap "$pkg" 2>&1 || true); echo "$out" | sed 's/^/    /'
  grep -q ' E: ' <<<"$out" && bad "namcap reported errors" || ok "no namcap errors"
fi

echo "install"
pacman -U --noconfirm "$pkg" >/dev/null && ok "pacman -U" || bad "pacman -U failed"
pocketdesk --version | grep -q '^pocketdesk [0-9]' && ok "$(pocketdesk --version)" || bad "pocketdesk --version"
for b in /usr/bin/pocketdesk /usr/bin/pocketdesk-app /usr/bin/pocketdesk-tray /usr/lib/pocketdesk/pocketdesk-pam-ask; do
  if ldd "$b" 2>&1 | grep -q 'not found'; then bad "$b is missing a library: $(ldd "$b" | grep 'not found' | tr -d '\t')"
  else ok "$b finds its libraries"; fi
done
command -v desktop-file-validate >/dev/null \
  && { desktop-file-validate /usr/share/applications/dev.pocketdesk.Pocketdesk.desktop && ok "launcher entry is valid" || bad "launcher entry invalid"; }
systemd-analyze verify --user /usr/lib/systemd/user/pocketdesk.service 2>/dev/null \
  && ok "user service verifies" || echo "  - systemd-analyze verify unavailable here (no user manager)"

echo
(( fails == 0 )) && echo "package ok" || { echo "$fails problem(s)"; exit 1; }
