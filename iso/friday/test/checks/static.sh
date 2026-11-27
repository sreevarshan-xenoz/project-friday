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

echo "FRIDAY Settings covers the commands (docs/SETTINGS-PAGES.md)"
SETTINGS_QML=$(cat "$REPO_ROOT"/packaging/linux/shell/settings/*.qml)
for need in '"voice", "speak"' '"say"' '"screen-text"' '"app-context"' '"sent"' '"forget"' '"setup-ai"' \
            '"desktop", "status"' '"tasks", "status"' '"phone", "status"' '"face", "status"' '"helper", "status"' '"doctor"' '"report"' '"daemon", "restart"' 'friday connect' 'friday private' '"feature", "list"' '"autonomy", "status"'; do
  if grep -qF -- "$need" <<<"$SETTINGS_QML"; then pass "Settings uses $need"; else fail "no FRIDAY Settings page uses $need"; fi
done
# Pairing a phone starts a server that lets it control this computer: the page
# runs `phone on --yes` only from its confirmation card, after showing what it allows.
PERM="$REPO_ROOT/packaging/linux/shell/settings/PermissionsPage.qml"
if [[ "$(grep -c 'phoneOn.run()' "$PERM")" == 1 ]] && grep 'phoneOn.run()' "$PERM" | grep -q 'confirmingPhone = false'; then
  pass "Settings runs 'phone on' only from its confirmation card"
else fail "Settings may run 'phone on' without the confirmation card"; fi
for page in Home AI Voice Permissions Privacy Features Health; do
  [[ -f "$REPO_ROOT/packaging/linux/shell/settings/${page}Page.qml" ]] && pass "page file $page" || fail "missing ${page}Page.qml"
done
grep -qF -- "Health & updates" "$REPO_ROOT/windows-tray/FridayTray/Fluent/SettingsWindow.xaml.cs" && pass "Windows has the Health & updates page" || fail "Windows lacks the Health & updates page"

echo "Hyprland Lua configs parse"
LUAC=$(command -v luac5.4 || command -v luac || true)
if [[ -n "$LUAC" ]]; then
  while IFS= read -r f; do
    expect "luac -p ${f#"$AIROOTFS"/}" "$LUAC" -p "$f"
  done < <(find "$AIROOTFS/etc/skel/.config/hypr" "$AIROOTFS/usr/share/friday/dots-hyprland/.config/hypr" -name '*.lua')
else skip "Lua syntax (no luac)"; fi

echo "Hyprland accepts the shipped configs (Hyprland --verify-config)"
# Hyprland refuses to run as root: as root (CI) the checks run as `nobody`,
# otherwise as the user running them.
unpriv=()
if [ "$(id -u)" = 0 ]; then command -v runuser >/dev/null 2>&1 && unpriv=(runuser -u nobody --) || unpriv=(false); fi
# Omarchy's own files come from fetch-omarchy.sh (not in git): without them
# the configs that load Omarchy can't be checked here.
omarchy_ok=false; [ -f "$AIROOTFS/usr/share/omarchy/default/hypr/bootstrap.lua" ] && omarchy_ok=true
if need Hyprland && [ "${unpriv[0]:-}" != false ]; then
  for cfg in "usr/share/friday/dots-hyprland/.config/hypr:Dots-Hyprland" "etc/skel/.config/hypr:Omarchy themes"; do
    src="$AIROOTFS/${cfg%%:*}"; label="${cfg##*:}"
    if [[ "$label" == Omarchy* ]] && ! $omarchy_ok; then skip "$label config (Omarchy not fetched: bash iso/friday/fetch-omarchy.sh)"; continue; fi
    # Outside $WORK: mktemp's 0700 directory is not reachable by `nobody`.
    H=$(mktemp -d /tmp/friday-hypr-verify.XXXXXX); chmod 755 "$H"
    mkdir -p "$H/.config"; cp -a "$src" "$H/.config/hypr"; chmod -R a+rwX "$H"
    out=$(cd "$H" && "${unpriv[@]}" env HOME="$H" XDG_RUNTIME_DIR="$H" \
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

echo "FRIDAY Shell can be used with a screen reader"
# Whatever takes keyboard focus says what it is; an icon-only button says
# what it does (its tooltip is its name).
nameless=""
while IFS= read -r f; do grep -q 'Accessible.role' "$f" || nameless+="${f#"$REPO_ROOT"/} "; done \
  < <(grep -rl 'activeFocusOnTab: true' "$REPO_ROOT/packaging/linux/shell")
[ -z "$nameless" ] && pass "every focusable control has an accessible role" || fail "no Accessible.role in: $nameless"
unnamed=$(grep -rn -A8 'IconButton {' "$REPO_ROOT/packaging/linux/shell" --include='*.qml' \
  | awk '/IconButton \{/ { if (n) print at; at = $0; n = 1; if ($0 ~ /tooltip:/) n = 0; next } /tooltip:/ { n = 0 } /^--$/ { if (n) print at; n = 0 } END { if (n) print at }' \
  | grep -v 'components/IconButton.qml' || true)
[ -z "$unnamed" ] && pass "every icon button has a name" || fail "icon buttons with no tooltip (name): $unnamed"

echo "FRIDAY Shell's look reaches Hyprland"
F_CLI="$REPO_ROOT/packaging/linux/friday"
THEME_QML="$REPO_ROOT/packaging/linux/shell/common/Theme.qml"
# The window borders use the palettes' accent and outline: the CLI's copy of
# them must match Theme.qml.
for pal in core ember polar frost moss dawn contrast; do
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
if ! $omarchy_ok; then skip "the window looks in Hyprland (Omarchy not fetched)"
elif need Hyprland jq && [ "${unpriv[0]:-}" != false ]; then
  for look in lean:core island:polar dock:omarchy; do
    H=$(mktemp -d /tmp/friday-hypr-look.XXXXXX); chmod 755 "$H"
    mkdir -p "$H/.config/friday"; cp -a "$AIROOTFS/etc/skel/.config/hypr" "$H/.config/hypr"
    printf '{ "style": "%s", "palette": "%s" }\n' "${look%%:*}" "${look##*:}" >"$H/.config/friday/shell.json"
    HOME="$H" XDG_CONFIG_HOME="" HYPRLAND_INSTANCE_SIGNATURE="" \
      bash -c 'source <(sed -n "/^shell_window_look() {/,/^}/p" "$1"); shell_window_look' _ "$F_CLI"
    chmod -R a+rwX "$H"
    out=$(cd "$H" && "${unpriv[@]}" env HOME="$H" XDG_RUNTIME_DIR="$H" \
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
   && grep -q 'jq -n --arg style "$SHELL_STYLE_ID"' "$AIROOTFS/root/installer"; then
  pass "installer saves the chosen style and writes the window look as the user"
else
  fail "installer doesn't save the style, or runs friday shell style before the user owns ~/.config/friday"
fi
for opt in "Lean (thin borders, small gaps, no blur" "FRIDAY Shell Core" "FRIDAY Shell Polar" "Island (floating pills along the top)" "Dock (floating bar along the bottom)" "Rail (a slim column down the left edge)" "Keystone (one notch in the middle of the top edge)" "Orbit (two floating pills, the middle left clear)" "FRIDAY Shell Dawn"; do
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
  for look in default lean:core island:polar dock:omarchy; do
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
grep -q "friday-mobile-\*.pkg.tar.zst" "$AIROOTFS/root/installer" \
  && pass "installer installs the friday-mobile package" || fail "installer does not install friday-mobile"
grep -q "packaging/arch/friday-mobile/PKGBUILD" "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh builds the friday-mobile package" || fail "build-iso.sh does not build friday-mobile"
grep -q "friday-wallpapers-\*.pkg.tar.zst" "$AIROOTFS/root/installer" \
  && pass "installer installs the wallpapers package" || fail "installer does not install friday-wallpapers"
grep -q "packaging/arch/wallpapers/PKGBUILD" "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh builds the wallpapers package" || fail "build-iso.sh does not build friday-wallpapers"
# The wallpapers are linked before omarchy-theme-set (which picks the first),
# with the chosen palette's set first: run the installer's lines on a fake root.
wp_line=$(grep -n '^  wp=/mnt/usr/share/friday/wallpapers' "$AIROOTFS/root/installer" | cut -d: -f1)
set_line=$(grep -n "omarchy-theme-set '\$THEME_SLUG'" "$AIROOTFS/root/installer" | head -1 | cut -d: -f1)
if [[ -n "$wp_line" && -n "$set_line" ]] && (( wp_line < set_line )); then
  pass "wallpapers are linked before the theme picks one"
else fail "wallpapers aren't linked before omarchy-theme-set"; fi
R=$(mktemp -d /tmp/friday-wp.XXXXXX)
mkdir -p "$R/usr/share/friday/wallpapers/core" "$R/usr/share/friday/wallpapers/polar" "$R/usr/share/friday/wallpapers/empty"
touch "$R/usr/share/friday/wallpapers/core/0-horizon.jpg" "$R/usr/share/friday/wallpapers/core/arcs.jpg" \
  "$R/usr/share/friday/wallpapers/polar/fjord.jpg"
snippet=$(sed -n '/^  wp=\/mnt\/usr\/share\/friday\/wallpapers/,/^  fi$/p' "$AIROOTFS/root/installer" | sed "s|/mnt|$R|g")
for pal in core polar; do
  ( arch-chroot() { :; }; USER_HOME=/home/u USERNAME=u THEME_SLUG=matte-black SHELL_PALETTE=$pal; eval "$snippet" )
  first=$(ls "$R/home/u/.config/omarchy/backgrounds/matte-black" | sort | head -1)
  want=$([[ $pal == core ]] && echo 0-core-0-horizon.jpg || echo 0-polar-fjord.jpg)
  n=$(ls "$R/home/u/.config/omarchy/backgrounds/matte-black" | wc -l)
  # Links point into the installed system (/usr/share/...), never the
  # installer's /mnt; an empty set links nothing.
  target=$(readlink "$R/home/u/.config/omarchy/backgrounds/matte-black/$first")
  want_target=/usr/share/friday/wallpapers/${want#0-}; want_target=${want_target/core-0-/core\/0-}; want_target=${want_target/polar-/polar\/}
  if [[ $first == "$want" && $n == 3 && $target == "$want_target" ]]; then pass "$pal: its own wallpaper comes first ($target), all $n linked"
  else fail "$pal: first wallpaper is $first -> $target (want $want -> $want_target), $n linked"; fi
  rm -rf "$R/home"
done
rm -rf "$R"
grep -q 'git clone -q "https://aur.archlinux.org/\$2.git"' "$AIROOTFS/root/installer" \
  && pass "installer builds Brave and Zen from the AUR (never Flathub)" || fail "installer does not build the AUR browsers"
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
# FRIDAY does what Omarchy's shell did; Omarchy's shell doesn't start.
grep -q '^omarchy_shell = false' "$AIROOTFS/etc/skel/.config/hypr/hyprland.lua" \
  && grep -q 'if _G.omarchy_shell ~= false then' "$PROFILE_DIR/omarchy-patches/0005-friday-own-shell.patch" \
  && pass "Omarchy's shell is switched off (hyprland.lua + patch 0005)" || fail "Omarchy's shell still starts"
for c in "friday wallpaper show" "friday idle"; do
  grep -q "o.exec_on_start(\"$c\")" "$AIROOTFS/etc/skel/.config/hypr/autostart.lua" \
    && pass "Hyprland autostarts $c" || fail "Hyprland does not start $c"
done
grep -q 'o.bind("SUPER + L", "Lock", "friday lock"' "$AIROOTFS/etc/skel/.config/hypr/bindings.lua" \
  && pass "Super+L locks with friday lock" || fail "Super+L doesn't run friday lock"
if grep -n 'omarchy-shell \|omarchy-menu \|omarchy-system-lock' "$AIROOTFS/etc/skel/.config/hypr/bindings.lua"; then
  fail "a key still calls Omarchy's shell, which doesn't run"
else pass "no key calls Omarchy's shell"; fi
for p in hyprlock hypridle swaybg; do
  grep -qw "$p" "$AIROOTFS/root/installer" && pass "installer installs $p" || fail "installer does not install $p"
done
if need jq; then
  # The installer's FRIDAY Shell settings: polkit on, the theme's wallpaper.
  line=$(grep -A2 'jq -n --arg style "$SHELL_STYLE_ID"' "$AIROOTFS/root/installer" | sed -n "s/^ *'\(.*\)' \\\\$/\1/p")
  out=$(jq -nc --arg style lean --arg palette core --arg wp /usr/share/friday/wallpapers/core/a.jpg "$line" 2>&1)
  [[ "$out" == '{"style":"lean","palette":"core","polkit":true,"wallpaper":"/usr/share/friday/wallpapers/core/a.jpg"}' ]] \
    && pass "installer turns on FRIDAY's password prompt and wallpaper" || fail "installer's shell.json: $out"
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

grep -q '^pacstrap_with_retry /mnt base linux linux-firmware intel-ucode amd-ucode ' "$AIROOTFS/root/installer" \
  && pass "installer installs CPU microcode" || fail "installer installs no CPU microcode"
grep -q '^chmod 700 "/mnt$USER_HOME"' "$AIROOTFS/root/installer" \
  && pass "installer keeps the home folder private" || fail "installer leaves the home folder readable by everyone"
# The live system's SSH: keys only, never an empty or any password.
sshd_live="$AIROOTFS/etc/ssh/sshd_config.d/10-archiso.conf"
if grep -qiE '^\s*(PermitEmptyPasswords\s+yes|PasswordAuthentication\s+yes|PermitRootLogin\s+yes)' "$sshd_live"; then
  fail "the live ISO's sshd takes passwords (anyone on the network can log in as root)"
else pass "the live ISO's sshd takes keys only"; fi
grep -q 'ssh.authorized_keys.root=' "$PROFILE_DIR/test/install-test.sh" \
  && pass "install-test hands the live system its key" || fail "install-test has no way into the live system"

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

# FRIDAY's browser extension: Chromium on FRIDAY OS loads it, and the package
# lets every browser find its host with no setup step.
contains "FRIDAY OS's Chromium loads FRIDAY's extension" "$(cat "$REPO_ROOT/iso/friday/airootfs/etc/skel/.config/chromium-flags.conf")" "/usr/share/friday/browser-extension"
contains "the package installs the host for Chromium" "$(cat "$REPO_ROOT/packaging/arch/PKGBUILD")" "etc/chromium/native-messaging-hosts"
contains "and for Firefox" "$(cat "$REPO_ROOT/packaging/arch/PKGBUILD")" "usr/lib/mozilla/native-messaging-hosts"
contains "the host manifest allows only FRIDAY's extension" "$(cat "$REPO_ROOT/packaging/browser/org.friday.browser.chromium.json")" "chrome-extension://eiblopljacafgajanhkngbhejpjjmfff/"
contains "and its Edge store copy" "$(cat "$REPO_ROOT/packaging/browser/org.friday.browser.chromium.json")" "chrome-extension://ncbhacgfkbbpmocppmagejakdnkikcpk/"

echo "no usage data leaves (telemetry off by default: there is none)"
# No analytics, crash-reporting or tracing-export SDK in what FRIDAY ships.
# Adding one needs a switch that starts off, and this list updated with it.
# Package names only (a crate's or NuGet package's name, split at - _ .),
# so a word inside another name or a version doesn't count.
TELEMETRY='(^|[-_.])(sentry|posthog|segment-analytics|analytics-rs|mixpanel|amplitude-analytics|applicationinsights|opentelemetry|datadog|bugsnag|rollbar|appcenter)([-_.]|$)'
if [[ -s "$REPO_ROOT/Cargo.lock" ]]; then
  names=$( { sed -nE 's/^name = "([^"]+)"/\1/p' "$REPO_ROOT/Cargo.lock"
             find "$REPO_ROOT/windows-host" "$REPO_ROOT/windows-tray" -name '*.csproj' -not -path '*/obj/*' -not -path '*Tests*' \
               -exec sed -nE 's/.*PackageReference Include="([^"]+)".*/\1/p' {} + ; } | tr 'A-Z' 'a-z')
  if grep -qE "$TELEMETRY" <<<"$names"; then fail "a telemetry SDK is a dependency: $(grep -E "$TELEMETRY" <<<"$names" | head -1)"
  else pass "no telemetry SDK among $(wc -l <<<"$names") shipped packages"; fi
else fail "Cargo.lock is missing: can't check for telemetry SDKs"; fi

echo "task recipes are the same on Linux and Windows"
rs_ids=$(sed -n '/pub const RECIPES/,/^];/p' "$REPO_ROOT/services/mobile/src/standing.rs" | sed -nE 's/.*id: "([^"]+)".*/\1/p' | sort)
cs_ids=$(sed -n '/Recipes =/,/\];/p' "$REPO_ROOT/windows-tray/FridayTray.Core/StandingTasks.cs" | sed -nE 's/.*\("([a-z-]+)",.*/\1/p' | sort)
if [[ -n "$rs_ids" && "$rs_ids" == "$cs_ids" ]]; then pass "both list: $(tr '\n' ' ' <<<"$rs_ids")"
else fail "recipes differ: daemon [$(tr '\n' ' ' <<<"$rs_ids")] tray [$(tr '\n' ' ' <<<"$cs_ids")]"; fi

finish
