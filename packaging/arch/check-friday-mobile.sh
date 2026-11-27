#!/usr/bin/env bash
# Smoke-test the built friday-mobile package: contents, that nothing is turned on,
# install, and that the installed binaries start and find their libraries.
# Run as root on Arch (it installs the package):
#   bash packaging/arch/check-friday-mobile.sh friday-mobile-*.pkg.tar.zst
set -euo pipefail
pkg=${1:?usage: check-friday-mobile.sh <package file>}
fails=0
ok()  { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fails=$((fails + 1)); }

echo "contents"
files=$(bsdtar -tf "$pkg")
for f in usr/bin/friday-mobile usr/bin/friday-mobile-app usr/bin/friday-mobile-tray \
  usr/lib/friday-mobile/friday-mobile-pam-ask usr/lib/systemd/user/friday-mobile.service \
  usr/share/applications/dev.friday.Mobile.desktop \
  usr/share/icons/hicolor/scalable/apps/dev.friday.Mobile.svg \
  usr/share/licenses/friday-mobile/LICENSE usr/share/licenses/friday-mobile/NOTICE; do
  grep -qx "$f" <<<"$files" && ok "$f" || bad "missing $f"
done
# makepkg compresses man pages, so the file may end in .gz.
grep -qE '^usr/share/man/man1/friday-mobile\.1(\.gz)?$' <<<"$files" && ok "the man page" || bad "missing the man page"
# Nothing of Pocketdesk's, so a computer with the pocketdesk package keeps it.
if grep -qi 'pocketdesk' <<<"$files"; then bad "a file is named after Pocketdesk"; else ok "no file is Pocketdesk's"; fi
bsdtar -xOf "$pkg" usr/share/applications/dev.friday.Mobile.desktop | grep -qx 'Icon=dev.friday.Mobile' \
  && ok "the launcher shows FRIDAY's icon" || bad "the launcher's Icon is not dev.friday.Mobile"
# Installed, not running: nothing may switch the service on, or touch PAM.
if grep -E '^(etc/|usr/lib/systemd/user/.*\.wants/|usr/lib/systemd/user-preset|usr/lib/pam\.d|usr/lib/security)' <<<"$files" >/dev/null; then
  bad "the package enables the service or touches /etc or PAM"
else
  ok "the service is installed but not enabled; /etc and PAM are untouched"
fi
bsdtar -xOf "$pkg" usr/lib/systemd/user/friday-mobile.service | grep -qx 'ExecStart=/usr/bin/friday-mobile start' \
  && ok "the unit starts /usr/bin/friday-mobile" || bad "the unit's ExecStart is not /usr/bin/friday-mobile start"

if command -v namcap >/dev/null; then
  echo "namcap"
  out=$(namcap "$pkg" 2>&1 || true); echo "$out" | sed 's/^/    /'
  grep -q ' E: ' <<<"$out" && bad "namcap reported errors" || ok "no namcap errors"
fi

echo "install"
pacman -U --noconfirm "$pkg" >/dev/null && ok "pacman -U" || bad "pacman -U failed"
friday-mobile --version | grep -q '^friday-mobile [0-9]' && ok "$(friday-mobile --version)" || bad "friday-mobile --version"
for b in /usr/bin/friday-mobile /usr/bin/friday-mobile-app /usr/bin/friday-mobile-tray /usr/lib/friday-mobile/friday-mobile-pam-ask; do
  if ldd "$b" 2>&1 | grep -q 'not found'; then bad "$b is missing a library: $(ldd "$b" | grep 'not found' | tr -d '\t')"
  else ok "$b finds its libraries"; fi
done
command -v desktop-file-validate >/dev/null \
  && { desktop-file-validate /usr/share/applications/dev.friday.Mobile.desktop && ok "launcher entry is valid" || bad "launcher entry invalid"; }
systemd-analyze verify --user /usr/lib/systemd/user/friday-mobile.service 2>/dev/null \
  && ok "user service verifies" || echo "  - systemd-analyze verify unavailable here (no user manager)"

echo
(( fails == 0 )) && echo "package ok" || { echo "$fails problem(s)"; exit 1; }
