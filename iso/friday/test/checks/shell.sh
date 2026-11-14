#!/usr/bin/env bash
# Load the desktop shells under headless sway and require each to load with no
# QML errors (a missing submodule and a missing Qt module once kept one from
# drawing anything):
#   - FRIDAY Shell (packaging/linux/shell), the default desktop: also opens
#     every drawer over IPC and sends a FRIDAY answer as a notification.
#   - The vendored Dots-Hyprland "ii" shell, still offered by the installer.
# Screenshots go to $WORK/shell/*.png.
source "$(dirname "$0")/lib.sh"
need sway qs grim || { skip "desktop shells (need sway, quickshell, grim)"; exit 0; }

W="$WORK/shell"; rm -rf "$W"; mkdir -p "$W/run"
export XDG_RUNTIME_DIR="$W/run" LANG=C.UTF-8
chmod 700 "$XDG_RUNTIME_DIR"

printf 'output HEADLESS-1 resolution 1600x900\n' >"$W/sway.conf"
# Arch's sway carries a file capability (cap_sys_nice). In a container whose
# bounding set lacks it (Docker's default, e.g. CI) exec fails with "Operation
# not permitted". A plain copy drops the capability; headless sway doesn't
# need it.
cp "$(command -v sway)" "$W/sway"
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 LIBSEAT_BACKEND=noop \
  "$W/sway" -c "$W/sway.conf" >"$W/sway.log" 2>&1 &
SWAY=$!
QS=""
trap 'kill $QS $SWAY 2>/dev/null; wait 2>/dev/null' EXIT
for _ in $(seq 40); do compgen -G "$XDG_RUNTIME_DIR/wayland-*" >/dev/null && break; sleep 0.25; done
sock=$(compgen -G "$XDG_RUNTIME_DIR/wayland-?" | head -n1)
if [[ -z "$sock" ]]; then fail "headless sway did not start (see $W/sway.log)"; finish; exit; fi
export WAYLAND_DISPLAY="$(basename "$sock")" QT_QPA_PLATFORM=wayland

# fresh_home <name>: an empty home with a FRIDAY state.json mid-goal.
fresh_home() {
  export HOME="$W/$1/home" XDG_STATE_HOME="$W/$1/home/.local/state"
  unset XDG_CONFIG_HOME
  mkdir -p "$HOME/.config" "$XDG_STATE_HOME/friday"
  echo '{"status":"running","current":{"active":true,"goal":"smoke test","phase":"plan","partial":"The answer is arriving word by word: E0308 means the types don'"'"'t match"},"last_goal":{"goal":"earlier","outcome":"completed","result":"ok"}}' \
    >"$XDG_STATE_HOME/friday/state.json"
}

# check_log <label> <log> [strict]: the shell loaded and logged no QML
# errors. strict (our own code) also rejects binding loops.
check_log() {
  if grep -q "Failed to load configuration" "$2"; then
    fail "$1 failed to load:"
    grep -E "caused by" "$2" | tail -n 4 | sed 's/^/      /'
  else
    pass "$1 loaded"
  fi
  local errs pat="TypeError|ReferenceError|Unable to assign|is not a type|unavailable"
  [[ "${3:-}" == strict ]] && pat="$pat|Binding loop|Cannot override"
  errs=$(grep -E "$pat" "$2" \
    | sed 's/\x1b\[[0-9;]*m//g' | sort | uniq -c || true)
  if [[ -n "$errs" ]]; then fail "$1: QML errors in the log:"; sed 's/^/      /' <<<"$errs"
  else pass "$1: no QML type or binding errors"; fi
}

notify() {
  [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && need notify-send || return 1
  timeout 5 notify-send -a FRIDAY "FRIDAY check" "notification popup test"
}

echo "FRIDAY Shell"
fresh_home friday
FS="$REPO_ROOT/packaging/linux/shell"
qs -p "$FS" >"$W/friday.log" 2>&1 &
QS=$!
sleep "${SHELL_WAIT:-12}"
if notify; then pass "notification accepted by FRIDAY Shell"; sleep 2
else skip "notification popup (no D-Bus session; run under dbus-run-session)"; fi
grim "$W/friday-bar.png" 2>/dev/null || true
# Workspaces off Hyprland come from the ext-workspace protocol (as on niri):
# sway has it, so its active workspace shows as an ember pill by the launcher.
if [[ -s "$W/friday-bar.png" ]] && need python3; then
  n=$(python3 "$(dirname "$0")/png_colour.py" "$W/friday-bar.png" "#ff8f4c" "36,6,120,30" 2>/dev/null || echo 0)
  if (( n > 40 )); then pass "workspaces show off Hyprland (ext-workspace, as on niri)"
  else fail "no workspace pill off Hyprland ($n ember px by the launcher)"; fi
fi
for d in friday control calendar launcher session; do
  if timeout 10 qs -p "$FS" ipc call shell open "$d" >/dev/null 2>&1; then
    sleep 1.5; grim "$W/friday-$d.png" 2>/dev/null || true; pass "drawer opens over IPC: $d"
  else
    fail "drawer did not open over IPC: $d"
  fi
done
timeout 10 qs -p "$FS" ipc call shell close >/dev/null 2>&1 || true
# The answer pill, handed an answer the way `friday ask` does.
answer=$'Your disk is mostly **photos** (41 GB) and the package cache (12 GB).\n\n- `~/Pictures`: 41 GB\n- `/var/cache/pacman`: 12 GB\n\nClear the cache with `sudo paccache -r`.'
if timeout 10 qs -p "$FS" ipc call answer present true "What's using my disk?" "$answer" "via OpenCode Zen · 1.4 s" >/dev/null 2>&1; then
  sleep 1.5; grim "$W/friday-answer.png" 2>/dev/null || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == answer ]] \
    && pass "answer pill shows an answer over IPC" || fail "answer pill did not take the answer"
  timeout 10 qs -p "$FS" ipc call answer dismiss >/dev/null 2>&1 || true
  sleep 1
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == idle ]] \
    && pass "answer pill dismisses" || fail "answer pill did not dismiss"
else
  fail "nothing answers the answer pill's IPC target"
fi
# Every style with a palette, switched live through shell.json as the Look
# picker does: the bar, the FRIDAY drawer and the control drawer each time.
mkdir -p "$HOME/.config/friday"
# The bar must really show the look: enough of the palette's background in
# the half of the screen the style puts the bar in (png_colour.py).
declare -A look_bg=([ember]=0d0f13 [frost]=0e141b [moss]=0f120e [dawn]=f7f3ee)
for look in island:frost dock:moss ember:dawn island:dawn dock:ember; do
  style=${look%%:*} palette=${look##*:}
  printf '{ "style": "%s", "palette": "%s" }\n' "$style" "$palette" >"$HOME/.config/friday/shell.json"
  sleep 3; grim "$W/look-${look/:/-}-bar.png" 2>/dev/null || true
  half=top; [[ $style == dock ]] && half=bottom
  if [[ -s "$W/look-${look/:/-}-bar.png" ]] && need python3; then
    n=$(python3 "$(dirname "$0")/png_colour.py" "$W/look-${look/:/-}-bar.png" "#${look_bg[$palette]}" "$half" 2>/dev/null || echo 0)
    if (( n > 3000 )); then pass "$style/$palette: the bar shows it (${n} px of #${look_bg[$palette]} at the $half)"
    else fail "$style/$palette: the bar doesn't show it (${n} px of #${look_bg[$palette]} at the $half)"; fi
  fi
  for d in friday control; do
    timeout 10 qs -p "$FS" ipc call shell open "$d" >/dev/null 2>&1 && sleep 1.5 \
      && grim "$W/look-${look/:/-}-$d.png" 2>/dev/null || true
  done
  timeout 10 qs -p "$FS" ipc call shell close >/dev/null 2>&1 || true
  kill -0 "$QS" 2>/dev/null || { fail "FRIDAY Shell exited switching to ${look} (see $W/friday.log)"; break; }
done
timeout 10 qs -p "$FS" ipc call notifications dismissAll >/dev/null 2>&1 \
  && pass "notifications IPC answers" || fail "notifications IPC did not answer"
# A malformed settings file must not break the shell.
mkdir -p "$HOME/.config/friday"; echo '{ not json' >"$HOME/.config/friday/shell.json"; sleep 2
check_log "FRIDAY Shell" "$W/friday.log" strict
if kill -0 "$QS" 2>/dev/null; then pass "FRIDAY Shell still running"; else fail "FRIDAY Shell exited (see $W/friday.log)"; fi
kill "$QS" 2>/dev/null; wait "$QS" 2>/dev/null; QS=""
[[ -s "$W/friday-bar.png" ]] && pass "screenshots: $W/friday-*.png, $W/look-*.png"

echo "Omarchy shell under the FRIDAY OS shell.json"
OM="$AIROOTFS/usr/share/omarchy"
if [[ -f "$OM/shell/shell.qml" ]] && need jq; then
  fresh_home omarchy
  # A disposable copy with FRIDAY's plugins from this tree, as build-iso.sh
  # ships them, and the matte-black theme applied (Omarchy reads its colours
  # from the generated shell.toml).
  cp -a "$OM" "$W/omarchy/tree"; rm -rf "$W/omarchy/tree/shell/plugins/"friday.*
  cp -r "$REPO_ROOT"/friday-os/plugins/friday.* "$W/omarchy/tree/shell/plugins/"
  T="$XDG_STATE_HOME/omarchy/current/theme"; mkdir -p "$T"; cp "$OM/themes/matte-black/colors.toml" "$T/"
  python3 - "$OM/default/themed/shell.toml.tpl" "$T/colors.toml" "$T/shell.toml" <<'PY'
import re, sys
tpl, colors, out = sys.argv[1:]
c = dict(re.findall(r'^\s*([a-z_]+)\s*=\s*"([^"]*)"', open(colors).read(), re.M))
open(out, "w").write(re.sub(r"\{\{\s*([a-z_]+)\s*\}\}", lambda m: c.get(m.group(1), "#000000"), open(tpl).read()))
PY
  mkdir -p "$HOME/.config/omarchy"
  # The installer's filter, read from the installer so the two can't drift.
  filter=$(sed -n "/if jq '.bar.id/,/unique)'/p" "$AIROOTFS/root/installer" | sed -e "s/^.*jq '//" -e "s/' \\\\$//" -e "s/'.*$//")
  jq "$filter" "$OM/config/omarchy/shell.json" >"$HOME/.config/omarchy/shell.json"
  OMARCHY_PATH="$W/omarchy/tree" qs -p "$W/omarchy/tree/shell" >"$W/omarchy.log" 2>&1 &
  QS=$!
  sleep "${SHELL_WAIT:-12}"
  plugins=$(timeout 10 qs -p "$W/omarchy/tree/shell" ipc call shell listPlugins 2>/dev/null || true)
  st() { jq -r --arg id "$1" --arg k "$2" '.[] | select(.id == $id) | .[$k]' <<<"$plugins" 2>/dev/null; }
  [[ "$(st friday.bar active)" == true && "$(st omarchy.bar active)" == false ]] \
    && pass "Omarchy's bar is off (friday.bar selected)" || fail "Omarchy's bar is still active"
  [[ "$(st omarchy.notifications enabled)" == false && "$(st omarchy.osd enabled)" == false ]] \
    && pass "Omarchy's notifications and OSD are off" || fail "Omarchy's notifications or OSD are still on"
  [[ "$(st omarchy.lock enabled)" == true && "$(st omarchy.polkit enabled)" == true ]] \
    && pass "Omarchy's lock screen and polkit agent stay on" || fail "Omarchy's lock screen or polkit agent is off"
  kill "$QS" 2>/dev/null; wait "$QS" 2>/dev/null; QS=""
  # FRIDAY's own Omarchy plugins (the bar widget and the context dialog) must
  # load cleanly too, for anyone who keeps Omarchy's bar: put the widget in
  # Omarchy's default bar and require no warnings from either plugin.
  jq '.bar.layout.right += [{"id": "friday.shell"}]' "$OM/config/omarchy/shell.json" >"$HOME/.config/omarchy/shell.json"
  OMARCHY_PATH="$W/omarchy/tree" qs -p "$W/omarchy/tree/shell" >"$W/omarchy-bar.log" 2>&1 &
  QS=$!
  sleep "${SHELL_WAIT:-12}"
  plugins=$(timeout 10 qs -p "$W/omarchy/tree/shell" ipc call shell listPlugins 2>/dev/null || true)
  [[ "$(st friday.shell enabled)" == true ]] && pass "friday.shell widget is in Omarchy's bar" \
    || fail "friday.shell widget did not register"
  warns=$(sed 's/\x1b\[[0-9;]*m//g' "$W/omarchy-bar.log" | grep -E "plugins/friday\.|friday\.shell failed" | sort -u || true)
  if [[ -z "$warns" ]]; then pass "FRIDAY's Omarchy plugins load without warnings"
  else fail "FRIDAY's Omarchy plugins log warnings:"; sed 's/^/      /' <<<"$warns"; fi
  kill "$QS" 2>/dev/null; wait "$QS" 2>/dev/null; QS=""
else
  skip "Omarchy shell (needs the staged Omarchy checkout and jq)"
fi

echo "Dots-Hyprland ii shell (installer option)"
fresh_home ii
cp -a "$AIROOTFS/usr/share/friday/dots-hyprland/.config/quickshell" "$HOME/.config/"
qs -p "$HOME/.config/quickshell/ii" >"$W/ii.log" 2>&1 &
QS=$!
sleep "${SHELL_WAIT:-20}"
# FRIDAY answers arrive as notifications; ii is the notification server here.
if notify; then pass "notification sent to the ii shell"; sleep 4
else skip "notification popup (no D-Bus session; run under dbus-run-session)"; fi
grim "$W/ii-bar.png" 2>/dev/null || true
check_log "ii shell" "$W/ii.log"
if kill -0 "$QS" 2>/dev/null; then pass "ii shell still running after ${SHELL_WAIT:-20}s"; else fail "ii shell exited (see $W/ii.log)"; fi

finish
