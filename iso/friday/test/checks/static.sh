#!/usr/bin/env bash
# Syntax, permissions and required files in the ISO profile.
source "$(dirname "$0")/lib.sh"

echo "shell scripts parse"
for f in "$AIROOTFS/root/configurator" "$AIROOTFS/root/installer" "$AIROOTFS/root/friday-tui.sh" \
         "$AIROOTFS/usr/bin/friday" "$PROFILE_DIR/build-iso.sh" "$PROFILE_DIR/fetch-omarchy.sh" \
         "$PROFILE_DIR/test/run-qemu.sh"; do
  expect "bash -n ${f#"$REPO_ROOT"/}" bash -n "$f"
done
if need zsh; then
  expect "zsh -n _friday completion" zsh -n "$AIROOTFS/usr/share/zsh/site-functions/_friday"
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

echo "executables are stored executable in git"
for f in usr/bin/friday usr/bin/friday-ctl usr/bin/friday-linux-daemon usr/bin/friday-skill-forge \
         root/configurator root/installer; do
  mode=$(git -C "$REPO_ROOT" ls-files -s "iso/friday/airootfs/$f" | awk '{print $1}')
  if [[ "$mode" == 100755 ]]; then pass "$f is 100755"; else fail "$f is ${mode:-untracked} (want 100755)"; fi
done

echo "files the desktop needs are present"
QS="$AIROOTFS/usr/share/friday/dots-hyprland/.config/quickshell/ii"
for f in "$QS/shell.qml" "$QS/services/Friday.qml" "$QS/modules/ii/bar/FridayIndicator.qml" \
         "$QS/modules/common/widgets/shapes/material-shapes.js" \
         "$AIROOTFS/usr/share/zsh/site-functions/_friday"; do
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

echo "Omarchy pin is a full SHA"
pin=$(sed -n 's/^OMARCHY_PIN="\([0-9a-f]*\)"/\1/p' "$PROFILE_DIR/fetch-omarchy.sh")
if [[ "$pin" =~ ^[0-9a-f]{40}$ ]]; then pass "pinned at ${pin:0:8}"; else fail "OMARCHY_PIN is not a 40-char SHA"; fi

finish
