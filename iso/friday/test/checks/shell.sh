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
  echo '{"status":"running","current":{"active":true,"goal":"smoke test","phase":"plan"},"last_goal":{"goal":"earlier","outcome":"completed","result":"ok"}}' \
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
for d in friday control calendar launcher session; do
  if timeout 10 qs -p "$FS" ipc call shell open "$d" >/dev/null 2>&1; then
    sleep 1.5; grim "$W/friday-$d.png" 2>/dev/null || true; pass "drawer opens over IPC: $d"
  else
    fail "drawer did not open over IPC: $d"
  fi
done
timeout 10 qs -p "$FS" ipc call shell close >/dev/null 2>&1 || true
timeout 10 qs -p "$FS" ipc call notifications dismissAll >/dev/null 2>&1 \
  && pass "notifications IPC answers" || fail "notifications IPC did not answer"
# A malformed settings file must not break the shell.
mkdir -p "$HOME/.config/friday"; echo '{ not json' >"$HOME/.config/friday/shell.json"; sleep 2
check_log "FRIDAY Shell" "$W/friday.log" strict
if kill -0 "$QS" 2>/dev/null; then pass "FRIDAY Shell still running"; else fail "FRIDAY Shell exited (see $W/friday.log)"; fi
kill "$QS" 2>/dev/null; wait "$QS" 2>/dev/null; QS=""
[[ -s "$W/friday-bar.png" ]] && pass "screenshots: $W/friday-*.png"

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
