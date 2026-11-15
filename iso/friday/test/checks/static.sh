#!/usr/bin/env bash
# Syntax, permissions and required files in the ISO profile.
source "$(dirname "$0")/lib.sh"

echo "shell scripts parse"
for f in "$AIROOTFS/root/configurator" "$AIROOTFS/root/installer" "$AIROOTFS/root/friday-tui.sh" \
         "$REPO_ROOT/packaging/linux/friday" "$PROFILE_DIR/build-iso.sh" "$PROFILE_DIR/fetch-omarchy.sh" \
         "$PROFILE_DIR/test/run-qemu.sh"; do
  expect "bash -n ${f#"$REPO_ROOT"/}" bash -n "$f"
done
if need zsh; then
  expect "zsh -n _friday completion" zsh -n "$REPO_ROOT/packaging/linux/_friday"
else skip "zsh completion syntax (no zsh)"; fi

echo "Hyprland Lua configs parse"
LUAC=$(command -v luac5.4 || command -v luac || true)
if [[ -n "$LUAC" ]]; then
  while IFS= read -r f; do
    expect "luac -p ${f#"$AIROOTFS"/}" "$LUAC" -p "$f"
  done < <(find "$AIROOTFS/etc/skel/.config/hypr" "$AIROOTFS/usr/share/friday/dots-hyprland/.config/hypr" -name '*.lua')
else skip "Lua syntax (no luac)"; fi

echo "Hyprland accepts the shipped configs (Hyprland --verify-config)"
if need Hyprland runuser; then
  for cfg in "usr/share/friday/dots-hyprland/.config/hypr:Dots-Hyprland" "etc/skel/.config/hypr:Omarchy themes"; do
    src="$AIROOTFS/${cfg%%:*}"; label="${cfg##*:}"
    # Outside $WORK: mktemp's 0700 directory is not reachable by `nobody`.
    H=$(mktemp -d /tmp/friday-hypr-verify.XXXXXX); chmod 755 "$H"
    mkdir -p "$H/.config"; cp -a "$src" "$H/.config/hypr"; chmod -R a+rwX "$H"
    # Hyprland refuses to run as root; verify as an unprivileged user.
    out=$(cd "$H" && runuser -u nobody -- env HOME="$H" XDG_RUNTIME_DIR="$H" \
      OMARCHY_PATH="$AIROOTFS/usr/share/omarchy" timeout 60 \
      Hyprland --verify-config -c "$H/.config/hypr/hyprland.lua" 2>&1)
    if grep -q "config ok" <<<"$out"; then pass "$label config"
    else
      fail "$label config:"
      if grep -q "Config parsing result" <<<"$out"; then sed -n '/Config parsing result/,$p' <<<"$out"
      else tail -n 5 <<<"$out"; fi | sed 's/^/      /'
    fi
    rm -rf "$H"
  done
else skip "Hyprland config verification (needs Hyprland)"; fi

echo "FRIDAY Shell's look reaches Hyprland"
F_CLI="$REPO_ROOT/packaging/linux/friday"
THEME_QML="$REPO_ROOT/packaging/linux/shell/common/Theme.qml"
# The window borders use the palettes' accent and outline: the CLI's copy of
# them must match Theme.qml.
for pal in ember frost moss dawn; do
  block=$(sed -n "/readonly property var $pal: ({/,/})/p" "$THEME_QML")
  acc=$(grep -oE 'accent: "#[0-9a-f]{6}"' <<<"$block" | grep -oE '[0-9a-f]{6}')
  out=$(grep -oE 'outline: "#[0-9a-f]{6}"' <<<"$block" | grep -oE '[0-9a-f]{6}')
  line=$(grep -E "^ +(\*\)\s+palette=)?$pal\)? .*accent=" "$F_CLI" | head -1)
  if [[ -n "$acc" && "$line" == *"accent=$acc outline=$out"* ]]; then pass "$pal: window borders match the shell ($acc / $out)"
  else fail "$pal: friday's hypr-look colours ($line) differ from Theme.qml ($acc / $out)"; fi
done
grep -q 'dofile(friday_look)' "$AIROOTFS/etc/skel/.config/hypr/hyprland.lua" \
  && pass "FRIDAY OS's hyprland.lua loads FRIDAY Shell's window look" \
  || fail "FRIDAY OS's hyprland.lua does not load ~/.config/friday/hypr-look.lua"
if need Hyprland runuser jq; then
  for look in island:frost dock:omarchy; do
    H=$(mktemp -d /tmp/friday-hypr-look.XXXXXX); chmod 755 "$H"
    mkdir -p "$H/.config/friday"; cp -a "$AIROOTFS/etc/skel/.config/hypr" "$H/.config/hypr"
    printf '{ "style": "%s", "palette": "%s" }\n' "${look%%:*}" "${look##*:}" >"$H/.config/friday/shell.json"
    HOME="$H" XDG_CONFIG_HOME="" HYPRLAND_INSTANCE_SIGNATURE="" \
      bash -c 'source <(sed -n "/^shell_window_look() {/,/^}/p" "$1"); shell_window_look' _ "$F_CLI"
    chmod -R a+rwX "$H"
    out=$(cd "$H" && runuser -u nobody -- env HOME="$H" XDG_RUNTIME_DIR="$H" \
      OMARCHY_PATH="$AIROOTFS/usr/share/omarchy" timeout 60 \
      Hyprland --verify-config -c "$H/.config/hypr/hyprland.lua" 2>&1)
    if [[ -s "$H/.config/friday/hypr-look.lua" ]] && grep -q "config ok" <<<"$out"; then
      pass "Hyprland accepts the ${look%%:*}/${look##*:} window look"
    else
      fail "${look%%:*}/${look##*:} window look:"; tail -n 5 <<<"$out" | sed 's/^/      /'
    fi
    rm -rf "$H"
  done
else skip "window look verification (needs Hyprland, jq)"; fi
own_line=$(grep -n 'chown -R "$USERNAME:$USERNAME" "$USER_HOME/.config/friday"' "$AIROOTFS/root/installer" | head -1 | cut -d: -f1)
style_line=$(grep -n "friday shell style '\$SHELL_STYLE_ID'" "$AIROOTFS/root/installer" | head -1 | cut -d: -f1)
if [[ -n "$own_line" && -n "$style_line" ]] && (( own_line < style_line )) \
   && grep -q '"style": "%s"' "$AIROOTFS/root/installer"; then
  pass "installer saves the chosen style and writes the window look as the user"
else
  fail "installer doesn't save the style, or runs friday shell style before the user owns ~/.config/friday"
fi
for opt in "Island (floating pills along the top)" "Dock (floating bar along the bottom)" "FRIDAY Shell Dawn"; do
  grep -qF "$opt" "$AIROOTFS/root/configurator" && pass "installer offers: $opt" || fail "installer no longer offers: $opt"
done

echo "FRIDAY OS on niri"
grep -qF '"niri (scrollable tiling' "$AIROOTFS/root/configurator" \
  && pass "installer offers niri" || fail "installer no longer offers niri"
grep -qE 'pacstrap_with_retry .*' "$AIROOTFS/root/installer" && grep -A8 'pacstrap_with_retry /mnt base' "$AIROOTFS/root/installer" | grep -q 'niri xwayland-satellite' \
  && pass "installer installs niri and xwayland-satellite" || fail "installer does not install niri and xwayland-satellite"
grep -q 'SESSION_FILE=niri.desktop' "$AIROOTFS/root/installer" \
  && pass "choosing niri starts niri (autologin and SDDM's preselected session)" || fail "choosing niri does not change the session"
NIRI_CONF="$AIROOTFS/etc/skel/.config/niri/config.kdl"
expect "the new user gets a default niri look" test -s "$AIROOTFS/etc/skel/.config/friday/niri-look.kdl"
if need niri jq; then
  for look in default island:frost dock:omarchy; do
    H=$(mktemp -d /tmp/friday-niri.XXXXXX)
    mkdir -p "$H/.config/niri" "$H/.config/friday"
    # The packaged FRIDAY snippet from this tree instead of /usr/share.
    sed "s|include \"/usr/share/friday/niri/friday.kdl\"|include \"$REPO_ROOT/packaging/linux/niri/friday.kdl\"|" \
      "$NIRI_CONF" >"$H/.config/niri/config.kdl"
    if [[ $look == default ]]; then
      cp "$AIROOTFS/etc/skel/.config/friday/niri-look.kdl" "$H/.config/friday/"
    else
      printf '{ "style": "%s", "palette": "%s" }\n' "${look%%:*}" "${look##*:}" >"$H/.config/friday/shell.json"
      HOME="$H" XDG_CONFIG_HOME="" HYPRLAND_INSTANCE_SIGNATURE="" \
        bash -c 'source <(sed -n "/^shell_window_look() {/,/^}/p" "$1"); shell_window_look' _ "$F_CLI"
    fi
    out=$(niri validate -c "$H/.config/niri/config.kdl" 2>&1)
    if grep -q "config is valid" <<<"$out"; then pass "niri accepts FRIDAY OS's config with the ${look/:/\/} look"
    else fail "niri rejects FRIDAY OS's config with the ${look/:/\/} look:"; grep -v DEBUG <<<"$out" | tail -n 6 | sed 's/^/      /'; fi
    rm -rf "$H"
  done
  grep -q 'spawn-at-startup "friday" "shell"' "$REPO_ROOT/packaging/linux/niri/friday.kdl" \
    && pass "niri starts FRIDAY Shell" || fail "niri does not start FRIDAY Shell"
  for key in 'Mod+Y ' 'Mod+Alt+Y ' 'Mod+Ctrl+Y '; do
    grep -q "^ *$key" "$REPO_ROOT/packaging/linux/niri/friday.kdl" && pass "niri binds $key" || fail "niri does not bind $key"
  done
elif [[ "${CI:-}" == "true" ]]; then
  fail "niri config validation (needs niri, jq)"
else skip "niri config validation (needs niri, jq)"; fi

echo "FRIDAY comes from its package, not loose files in airootfs"
# pacman refuses to install over files it doesn't own, so these must not be in
# airootfs (the friday-git package installs them).
for f in usr/bin/friday usr/bin/friday-ctl usr/bin/friday-linux-daemon usr/lib/friday \
         usr/lib/systemd/user/friday-linux-daemon.service etc/systemd/user/friday-linux-daemon.service \
         usr/share/zsh/site-functions/_friday usr/share/applications/friday.desktop usr/share/friday/hyprland; do
  if [[ -e "$AIROOTFS/$f" ]]; then fail "airootfs/$f would conflict with the friday-git package"
  else pass "$f is left to the package"; fi
done
grep -qx 'friday-git' "$PROFILE_DIR/packages.x86_64" && pass "live ISO installs friday-git" \
  || fail "friday-git is not in packages.x86_64"
grep -q 'pacstrap_with_retry -U /mnt "$FRIDAY_PKG"' "$AIROOTFS/root/installer" \
  && pass "installer installs the friday-git package" || fail "installer does not install friday-git"
grep -q "pocketdesk-\*.pkg.tar.zst" "$AIROOTFS/root/installer" \
  && pass "installer installs the pocketdesk package" || fail "installer does not install pocketdesk"
grep -q "packaging/arch/pocketdesk/PKGBUILD" "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh builds the pocketdesk package" || fail "build-iso.sh does not build pocketdesk"
# The update channel: the installer's [friday] repo must be where release.yml
# publishes (the rolling "pacman" release).
if grep -q '^Server = https://github.com/sreevarshan-xenoz/project-friday/releases/download/pacman$' "$AIROOTFS/root/installer" \
   && grep -q 'gh release upload pacman ' "$REPO_ROOT/.github/workflows/release.yml"; then
  pass "installed systems update FRIDAY from the release workflow's pacman repo"
else fail "the installer's [friday] repo and release.yml's pacman release don't match"; fi
# Updates are signed: installed systems require FRIDAY's release key, which
# friday-git ships; the committed fingerprint must be that key's.
if grep -q '^SigLevel = Required DatabaseRequired$' "$AIROOTFS/root/installer" \
   && grep -q 'pacman-key --lsign-key' "$AIROOTFS/root/installer"; then
  pass "installed systems require FRIDAY's signature on updates"
else fail "the installer's [friday] repo doesn't require signatures"; fi
key="$REPO_ROOT/packaging/arch/friday-signing-key.asc"
if need gpg && [[ -f "$key" ]]; then
  shown=$(gpg --batch --with-colons --show-keys "$key" 2>/dev/null | awk -F: '/^fpr/{print $10; exit}')
  if [[ -n "$shown" && "$shown" == "$(cat "$REPO_ROOT/packaging/arch/friday-signing-key.fpr")" ]] \
     && grep -q 'friday-signing-key.asc' "$REPO_ROOT/packaging/arch/PKGBUILD"; then
    pass "the release key's fingerprint matches and friday-git ships the key"
  else fail "packaging/arch/friday-signing-key.{asc,fpr} disagree or friday-git doesn't ship them"; fi
else skip "release key check (needs gpg)"; fi
# bash sets $HOSTNAME to the live system's name, so an unattended install that
# defaulted from it was named "archiso" (found in a VM install, 2026-11-14).
if (unset FRIDAY_HOSTNAME; HOSTNAME=archiso; eval "$(grep -m1 '^HOSTNAME=' "$AIROOTFS/root/installer")"; [[ $HOSTNAME == friday ]]); then
  pass "unattended install names the computer friday, not the live system's hostname"
else fail "installer takes its hostname default from bash's \$HOSTNAME"; fi
# `friday` is the CLI (packaging/linux/friday); an alias to friday-ctl in the
# default shell config sent `friday connect` / `setup-ai` / `doctor` typed in a
# terminal to the wrong program (found recording a demo, 2026-11-14).
if grep -rqsE "^[[:space:]]*alias[[:space:]]+friday=" "$AIROOTFS/etc/skel"; then
  fail "etc/skel aliases friday, which shadows the friday CLI"
else pass "etc/skel leaves the friday command alone"; fi
# Hyprland 0.57 deprecates the on_created_empty workspace rule and shows a
# banner on every login; omarchy-patches/ backports upstream's replacement.
if [[ -d "$AIROOTFS/usr/share/omarchy/default/hypr" ]]; then
  if grep -rqsE "on_created_empty[[:space:]]*=" "$AIROOTFS/usr/share/omarchy/default/hypr"; then
    fail "staged Omarchy still uses the deprecated on_created_empty workspace rule"
  else pass "staged Omarchy has no deprecated on_created_empty rule"; fi
fi

echo "scripts are stored executable in git"
for f in packaging/linux/friday iso/friday/airootfs/root/configurator iso/friday/airootfs/root/installer; do
  mode=$(git -C "$REPO_ROOT" ls-files -s "$f" | awk '{print $1}')
  if [[ "$mode" == 100755 ]]; then pass "$f is 100755"; else fail "$f is ${mode:-untracked} (want 100755)"; fi
done

echo "files the desktop needs are present"
QS="$AIROOTFS/usr/share/friday/dots-hyprland/.config/quickshell/ii"
for f in "$QS/shell.qml" "$QS/services/Friday.qml" "$QS/modules/ii/bar/FridayIndicator.qml" \
         "$QS/modules/common/widgets/shapes/material-shapes.js"; do
  expect "${f#"$AIROOTFS"/}" test -s "$f"
done
# Every relative QML import directory must exist (a lost git submodule broke the shell once).
count=$(grep -rhoE '^import "[^"]+"' "$QS" --include='*.qml' 2>/dev/null | sort -u | wc -l)
pass "scanned $count distinct relative QML imports"
while IFS=: read -r file imp; do
  dir=$(dirname "$file"); target=$(sed -E 's/^import "([^"]+)".*/\1/' <<<"$imp")
  [[ -e "$dir/$target" ]] || fail "QML import $target missing (from ${file#"$QS"/})"
done < <(grep -rHoE '^import "[^"]+"' "$QS" --include='*.qml')
# Module-style imports of the shell itself (`import qs.modules.x.y`) map to directories.
while IFS= read -r mod; do
  [[ -d "$QS/$(tr . / <<<"${mod#qs.}")" ]] || fail "QML module $mod has no directory"
done < <(grep -rhoE '^import qs(\.[A-Za-z0-9_]+)+' "$QS" --include='*.qml' | awk '{print $2}' | sort -u)
pass "shell-internal QML modules resolve"

echo "FRIDAY Shell is the default desktop"
FSH="$REPO_ROOT/packaging/linux/shell"
expect "packaging/linux/shell/shell.qml" test -s "$FSH/shell.qml"
while IFS= read -r mod; do
  [[ -d "$FSH/$(tr . / <<<"${mod#qs.}")" ]] || fail "FRIDAY Shell module $mod has no directory"
done < <(grep -rhoE '^import qs(\.[A-Za-z0-9_]+)+' "$FSH" --include='*.qml' | awk '{print $2}' | sort -u)
pass "FRIDAY Shell's internal QML modules resolve"
grep -q 'THEME="${THEME:-FRIDAY Shell' "$AIROOTFS/root/installer" \
  && pass "installer defaults to FRIDAY Shell" || fail "installer does not default to FRIDAY Shell"
grep -q '^THEME="FRIDAY Shell' "$AIROOTFS/root/configurator" \
  && pass "configurator defaults to FRIDAY Shell" || fail "configurator does not default to FRIDAY Shell"
grep -q 'Dots-Hyprland' "$AIROOTFS/root/configurator" \
  && pass "end-4 Dots-Hyprland is still offered" || fail "end-4 Dots-Hyprland option is gone"
grep -q 'o.exec_on_start("friday shell")' "$AIROOTFS/etc/skel/.config/hypr/autostart.lua" \
  && pass "Hyprland autostarts FRIDAY Shell" || fail "Hyprland does not start FRIDAY Shell"
grep -q '.bar.id = "friday.bar"' "$AIROOTFS/root/installer" && grep -q 'omarchy.notifications' "$AIROOTFS/root/installer" \
  && pass "installer turns off Omarchy's bar and notifications" || fail "installer leaves Omarchy's bar or notifications on"
if need jq; then
  expect "friday.bar manifest is valid JSON" jq -e '.id == "friday.bar" and (.kinds | index("bar"))' \
    "$REPO_ROOT/friday-os/plugins/friday.bar/manifest.json"
fi
grep -q 'OMARCHY_PATH=/usr/share/omarchy PATH=/usr/share/omarchy/bin:\\$PATH omarchy-theme-set' "$AIROOTFS/root/installer" \
  && pass "installer runs omarchy-theme-set with Omarchy's environment" \
  || fail "installer runs omarchy-theme-set without OMARCHY_PATH/PATH (theme silently not applied)"
chown_line=$(grep -n 'chown -R "$USERNAME:$USERNAME" "$USER_HOME"' "$AIROOTFS/root/installer" | head -1 | cut -d: -f1)
theme_line=$(grep -n 'omarchy-theme-set .\$THEME_SLUG' "$AIROOTFS/root/installer" | head -1 | cut -d: -f1)
if [[ -n "$chown_line" && -n "$theme_line" ]] && (( chown_line < theme_line )); then
  pass "installer hands the home to the user before the theme step"
else
  fail "installer runs omarchy-theme-set before the user owns their home (theme silently not applied)"
fi
grep -q 'friday-os/plugins/friday.bar' "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh ships the friday.bar plugin" || fail "build-iso.sh does not ship friday.bar"
grep -q 'friday-os/plugins/friday.lock' "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh ships the friday.lock plugin" || fail "build-iso.sh does not ship friday.lock"
grep -q 'omarchy-apply-lock' "$AIROOTFS/root/installer" \
  && pass "installer sets up the lock screen's PAM service" || fail "installer never writes /etc/pam.d/omarchy-lock-password (the lock screen refuses to lock)"
grep -q '"omarchy.lock"' "$AIROOTFS/root/installer" \
  && pass "installer turns off Omarchy's lock screen for friday.lock" || fail "installer leaves omarchy.lock on next to friday.lock"
if need jq; then
  expect "friday.lock manifest is valid JSON" jq -e '.id == "friday.lock" and (.kinds | index("service"))' \
    "$REPO_ROOT/friday-os/plugins/friday.lock/manifest.json"
fi

echo "Omarchy commands stay executable in the ISO"
if [[ -x "$AIROOTFS/usr/share/omarchy/bin/omarchy-theme-set" ]]; then
  # mkarchiso drops file modes; profiledef.sh must list every executable.
  perms=$(cd "$PROFILE_DIR" && bash -c 'declare -A file_permissions; . ./profiledef.sh; printf "%s\n" "${!file_permissions[@]}"')
  missing=0
  while IFS= read -r f; do
    grep -qxF "/${f#"$PROFILE_DIR/airootfs/"}" <<<"$perms" || missing=$((missing + 1))
  done < <(find "$AIROOTFS/usr/share/omarchy" -type f -perm -u+x)
  if (( missing == 0 )); then pass "profiledef.sh keeps every Omarchy executable executable"
  else fail "$missing Omarchy executables would lose their executable bit in the ISO"; fi
else
  skip "Omarchy executables (no staged Omarchy checkout)"
fi

echo "Omarchy pin is a full SHA"
pin=$(sed -n 's/^OMARCHY_PIN="\([0-9a-f]*\)"/\1/p' "$PROFILE_DIR/fetch-omarchy.sh")
if [[ "$pin" =~ ^[0-9a-f]{40}$ ]]; then pass "pinned at ${pin:0:8}"; else fail "OMARCHY_PIN is not a 40-char SHA"; fi

finish

# Setup runs on the Linux console in Terminus: every glyph it draws there must
# be in the font, or it shows as a box (the old wordmark's ╗║═ did). Glyphs
# used only off the console (✓ ✗ ❯, rounded corners, the Braille spinner) are
# exempt.
font="/usr/share/kbd/consolefonts/ter-v24b.psf.gz"
if need psfgettable python3 && [[ -f "$font" ]]; then
  psf=$(mktemp); zcat "$font" >"$psf"
  missing=$(python3 - "$psf" "$AIROOTFS/root" <<'PY'
import re, subprocess, sys
table = subprocess.run(["psfgettable", sys.argv[1]], capture_output=True, text=True).stdout
have = {int(h, 16) for h in re.findall(r"U\+([0-9A-Fa-f]{4,5})", table)}
exempt = set("✓✗❯╭╮╰╯") | {chr(c) for c in range(0x2800, 0x2900)}
seen = {}
for name in ("friday-tui.sh", "configurator", "installer"):
    for n, line in enumerate(open(f"{sys.argv[2]}/{name}", encoding="utf-8"), 1):
        if line.lstrip().startswith("#"):
            continue
        for ch in line:
            if ord(ch) > 127 and ch not in exempt and ord(ch) not in have:
                seen.setdefault(ch, f"{name}:{n}")
print(" ".join(f"{c}({w})" for c, w in seen.items()))
PY
)
  rm -f "$psf"
  if [[ -z "$missing" ]]; then pass "setup draws only glyphs the console font (Terminus) has"
  else fail "setup draws glyphs Terminus lacks: $missing"; fi
else skip "console glyph check (needs psfgettable, python3, terminus-font)"; fi
