#!/usr/bin/env bash
# Load the shipped Dots-Hyprland "ii" Quickshell config under headless sway
# and require it to load with no QML errors (a missing submodule and a missing
# Qt module once kept it from drawing anything). Saves a screenshot to
# $WORK/shell/bar.png.
source "$(dirname "$0")/lib.sh"
need sway qs grim || { skip "desktop shell (needs sway, quickshell, grim)"; exit 0; }

W="$WORK/shell"; rm -rf "$W"; mkdir -p "$W/home/.config" "$W/run"
export HOME="$W/home" XDG_RUNTIME_DIR="$W/run" XDG_STATE_HOME="$W/home/.local/state"
chmod 700 "$XDG_RUNTIME_DIR"
cp -a "$AIROOTFS/usr/share/friday/dots-hyprland/.config/quickshell" "$HOME/.config/"
mkdir -p "$XDG_STATE_HOME/friday"
echo '{"status":"running","current":{"active":true,"goal":"smoke test","phase":"planning"}}' \
  >"$XDG_STATE_HOME/friday/state.json"

printf 'output HEADLESS-1 resolution 1600x900\n' >"$W/sway.conf"
# Arch's sway carries a file capability (cap_sys_nice). In a container whose
# bounding set lacks it (Docker's default, e.g. CI) exec fails with "Operation
# not permitted". A plain copy drops the capability; headless sway doesn't
# need it.
cp "$(command -v sway)" "$W/sway"
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 LIBSEAT_BACKEND=noop \
  "$W/sway" -c "$W/sway.conf" >"$W/sway.log" 2>&1 &
SWAY=$!
trap 'kill $QS $SWAY 2>/dev/null; wait 2>/dev/null' EXIT
for _ in $(seq 40); do compgen -G "$XDG_RUNTIME_DIR/wayland-*" >/dev/null && break; sleep 0.25; done
sock=$(compgen -G "$XDG_RUNTIME_DIR/wayland-?" | head -n1)
if [[ -z "$sock" ]]; then fail "headless sway did not start (see $W/sway.log)"; finish; exit; fi
export WAYLAND_DISPLAY="$(basename "$sock")" QT_QPA_PLATFORM=wayland

qs -p "$HOME/.config/quickshell/ii" >"$W/qs.log" 2>&1 &
QS=$!
sleep "${SHELL_WAIT:-20}"
# FRIDAY answers arrive as notifications; ii is the notification server.
if [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && need notify-send; then
  notify-send -a FRIDAY "FRIDAY check" "notification popup test" && sleep 4
  NOTIFIED=1
fi
grim "$W/bar.png" 2>/dev/null || true

if grep -q "Failed to load configuration" "$W/qs.log"; then
  fail "ii shell failed to load:"
  grep -E "caused by" "$W/qs.log" | tail -n 4 | sed 's/^/      /'
else
  pass "ii shell loaded"
fi
# Any QML type/binding error anywhere in the shell (not only FRIDAY's widget).
errs=$(grep -E "TypeError|ReferenceError|Unable to assign|is not a type|unavailable" "$W/qs.log" \
  | sed 's/\x1b\[[0-9;]*m//g' | sort | uniq -c || true)
if [[ -n "$errs" ]]; then fail "QML errors in the shell log:"; sed 's/^/      /' <<<"$errs"
else pass "no QML type or binding errors"; fi
if [[ "${NOTIFIED:-0}" == 1 ]]; then pass "notification sent to the shell (see screenshot)"
else skip "notification popup (no D-Bus session; run under dbus-run-session)"; fi
if kill -0 "$QS" 2>/dev/null; then pass "shell still running after ${SHELL_WAIT:-20}s"; else fail "shell exited (see $W/qs.log)"; fi
[[ -s "$W/bar.png" ]] && pass "screenshot: $W/bar.png"

finish
