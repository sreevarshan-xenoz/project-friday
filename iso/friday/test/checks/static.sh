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
grep -q 'friday-os/plugins/friday.bar' "$PROFILE_DIR/build-iso.sh" \
  && pass "build-iso.sh ships the friday.bar plugin" || fail "build-iso.sh does not ship friday.bar"

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
