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
if timeout 10 qs -p "$FS" ipc call answer present true "What's using my disk?" "$answer" "via OpenCode Zen · 1.4 s" "" "opencode/big-pickle" >/dev/null 2>&1; then
  sleep 1.5; grim "$W/friday-answer.png" 2>/dev/null || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == answer ]] \
    && pass "answer pill shows an answer over IPC" || fail "answer pill did not take the answer"
  timeout 10 qs -p "$FS" ipc call answer listening >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == listening ]] \
    && pass "answer pill shows Listening while Super+Y is held" || fail "answer pill has no listening state"
  timeout 10 qs -p "$FS" ipc call answer dictating >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == listening ]] \
    && pass "answer pill shows Listening while Super+Shift+Y (voice typing) is held" || fail "answer pill has no dictating state"
  timeout 10 qs -p "$FS" ipc call answer dismiss >/dev/null 2>&1 || true
  sleep 1
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == idle ]] \
    && pass "answer pill dismisses" || fail "answer pill did not dismiss"
else
  fail "nothing answers the answer pill's IPC target"
fi
# The Settings AI page must never delete saved keys by itself: saving keeps the
# others, and the one --only (back to the free models) sits behind a confirm.
AI_PAGE="$FS/settings/AIPage.qml"
if grep -q -- '--keep-others' "$AI_PAGE" && [[ "$(grep -c -- '--only' "$AI_PAGE")" == 1 ]] && grep -q 'confirmingFree' "$AI_PAGE"; then
  pass "Settings AI page keeps saved keys unless the free-models confirm is accepted"
else fail "Settings AI page may delete saved keys without asking"; fi
# FRIDAY Settings: opens on a page, shows it, closes.
timeout 10 qs -p "$FS" ipc call settings open Voice >/dev/null 2>&1 || true
sleep 1.5; grim "$W/friday-settings.png" 2>/dev/null || true
[[ "$(timeout 10 qs -p "$FS" ipc call settings page 2>/dev/null)" == Voice ]] && pass "FRIDAY Settings opens on the Voice page" || fail "FRIDAY Settings did not open"
timeout 10 qs -p "$FS" ipc call settings open AI >/dev/null 2>&1 || true
sleep 1; grim "$W/friday-settings-ai.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call settings open Privacy >/dev/null 2>&1 || true
sleep 1; grim "$W/friday-settings-privacy.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call settings open Permissions >/dev/null 2>&1 || true
sleep 2; grim "$W/friday-settings-permissions.png" 2>/dev/null || true
for p in Home AI Permissions Privacy Health; do
  timeout 10 qs -p "$FS" ipc call settings open "$p" >/dev/null 2>&1 || true
  sleep 0.5
  [[ "$(timeout 10 qs -p "$FS" ipc call settings page 2>/dev/null)" == "$p" ]] || fail "FRIDAY Settings page $p did not open"
done
# The pages can be switched from the keyboard: Tab to the second entry, Enter.
if need wtype; then
  timeout 10 qs -p "$FS" ipc call settings open Home >/dev/null 2>&1 || true
  sleep 1
  wtype -s 400 -k Tab -s 400 -k Tab -s 400 -k Return 2>/dev/null || true
  sleep 0.7
  [[ "$(timeout 10 qs -p "$FS" ipc call settings page 2>/dev/null)" == AI ]] \
    && pass "FRIDAY Settings pages switch from the keyboard (Tab, Enter)" || fail "FRIDAY Settings ignored Tab and Enter"
else skip "keyboard use of FRIDAY Settings (needs wtype)"; fi
if grep -q "settings/.*File not found" "$W/friday.log"; then fail "a FRIDAY Settings page file is missing: $(grep -h 'settings/.*File not found' "$W/friday.log" | head -1)"
else pass "every FRIDAY Settings page file loads"; fi
timeout 10 qs -p "$FS" ipc call settings close >/dev/null 2>&1 || true
[[ -z "$(timeout 10 qs -p "$FS" ipc call settings page 2>/dev/null)" ]] && pass "FRIDAY Settings closes" || fail "FRIDAY Settings did not close"
# The first-login tour: begins, moves on when FRIDAY answers, can be skipped.
timeout 10 qs -p "$FS" ipc call tour begin "Alex" true false >/dev/null 2>&1 || true
sleep 1.5; grim "$W/friday-tour.png" 2>/dev/null || true
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 0 ]] && pass "tour begins at its first step" || fail "tour did not begin"
timeout 10 qs -p "$FS" ipc call answer present true "q" "a" "" "" "" >/dev/null 2>&1 || true
sleep 0.5
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 1 ]] && pass "tour moves on when FRIDAY answers" || fail "tour did not move on after an answer"
timeout 10 qs -p "$FS" ipc call answer dismiss >/dev/null 2>&1 || true
# A failed answer doesn't count: the step stays, so the first question that went wrong isn't skipped past.
timeout 10 qs -p "$FS" ipc call answer present false "q" "it failed" "" "" "" >/dev/null 2>&1 || true
sleep 0.5
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 1 ]] && pass "tour stays on a step when the answer failed" || fail "tour moved on after a failed answer"
timeout 10 qs -p "$FS" ipc call answer dismiss >/dev/null 2>&1 || true
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == -1 ]] && pass "tour can be skipped" || fail "tour did not finish"
# With Pocketdesk installed the tour has a last step about the phone.
timeout 10 qs -p "$FS" ipc call tour begin "Alex" true true >/dev/null 2>&1 || true
for _ in 1 2 3 4 5; do timeout 10 qs -p "$FS" ipc call tour next >/dev/null 2>&1 || true; done
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 5 ]] && pass "tour has a phone step when Pocketdesk is installed" || fail "tour has no phone step"
sleep 1; grim "$W/friday-tour-phone.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call tour back >/dev/null 2>&1 || true
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 4 ]] && pass "tour can go back a step" || fail "tour did not go back"
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
# Past answers, as friday ask writes them, are searchable.
printf '# what is using my disk\n\n_2026-11-14 21:10_\n\nMostly photos.\n\n---\n\n# explain rust error E0308\n\n_2026-11-14 22:30_\n\n> fn main\n\nMismatched types.\n\n---\n\n' \
  >"$XDG_STATE_HOME/friday/answers.md"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call history count 2>/dev/null)" == 2 ]] \
  && pass "answer history reads past answers" || fail "answer history did not read answers.md"
[[ "$(timeout 10 qs -p "$FS" ipc call history find "mismatched rust" 2>/dev/null)" == "explain rust error E0308" ]] \
  && pass "answer history finds an answer by its words" || fail "answer history search found nothing"
# New entries carry a marker, so an answer with a "---" rule stays one entry.
printf '<!-- friday answer -->\n# compare tabs and spaces\n\n_2026-11-15 01:00_\n\nTabs first.\n\n---\n\nThen spaces.\n\n---\n\n' \
  >>"$XDG_STATE_HOME/friday/answers.md"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call history count 2>/dev/null)" == 3 ]] \
  && pass "an answer containing --- stays one history entry" || fail "a --- inside an answer split it ($(timeout 10 qs -p "$FS" ipc call history count 2>/dev/null) entries)"
[[ "$(timeout 10 qs -p "$FS" ipc call history find "then spaces" 2>/dev/null)" == "compare tabs and spaces" ]] \
  && pass "the text after a --- is still found" || fail "the text after a --- was lost"
# Standing tasks, as the daemon keeps them, listed in the FRIDAY drawer.
printf '%s' '{"tasks":[{"id":"t1","goal":"tell me the weather","trigger":{"kind":"daily","hour":8,"minute":0,"weekdays_only":true},"paused":true,"created":1},{"id":"t2","goal":"say when the battery is low","trigger":{"kind":"battery_below","percent":20},"created":2}]}' \
  >"$XDG_STATE_HOME/friday/standing.json"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call tasks count 2>/dev/null)" == 2 ]] \
  && pass "standing tasks are read from standing.json" || fail "standing tasks were not read"
[[ "$(timeout 10 qs -p "$FS" ipc call tasks first 2>/dev/null)" == "every weekday at 08:00 | tell me the weather | paused" ]] \
  && pass "a standing task shows its schedule, goal and paused state" || fail "standing task text wrong: $(timeout 10 qs -p "$FS" ipc call tasks first 2>/dev/null)"
printf '%s' 'not json' >"$XDG_STATE_HOME/friday/standing.json"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call tasks count 2>/dev/null)" == 0 ]] \
  && pass "an unreadable standing.json lists nothing" || fail "an unreadable standing.json broke the list"
rm -f "$XDG_STATE_HOME/friday/standing.json"
# What FRIDAY did on the desktop, as the daemon logs it (actions.jsonl), shown in the drawer.
printf '%s\n' \
  '{"ts":"2026-11-18T10:00:00Z","verb":"focus_window","params":{"target":"firefox"},"risk":"reversible","ok":true,"goal":"x"}' \
  '{"ts":"2026-11-18T10:01:00Z","verb":"close_window","params":{"target":"gedit"},"risk":"destructive","ok":false,"goal":"(confirmed)"}' \
  '{"ts":"2026-11-18T10:02:0' >"$XDG_STATE_HOME/friday/actions.jsonl"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call actions count 2>/dev/null)" == 2 ]] \
  && pass "the action log is read (a torn last line is skipped)" || fail "action log count wrong: $(timeout 10 qs -p "$FS" ipc call actions count 2>/dev/null)"
[[ "$(timeout 10 qs -p "$FS" ipc call actions first 2>/dev/null)" == "close window · gedit | failed" ]] \
  && pass "the newest action shows what it did and that it failed" || fail "action text wrong: $(timeout 10 qs -p "$FS" ipc call actions first 2>/dev/null)"
rm -f "$XDG_STATE_HOME/friday/actions.jsonl"
sleep 1
[[ "$(timeout 10 qs -p "$FS" ipc call actions count 2>/dev/null)" == 0 ]] \
  && pass "no action log lists nothing" || fail "a missing action log still listed actions"
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
  [[ "$(st omarchy.lock enabled)" == false && "$(st friday.lock enabled)" == true ]] \
    && pass "FRIDAY's lock screen replaces Omarchy's" || fail "lock screen plugins: omarchy.lock=$(st omarchy.lock enabled) friday.lock=$(st friday.lock enabled)"
  [[ "$(jq -r '.idle.screensaver' "$HOME/.config/omarchy/shell.json")" == 86400 ]] \
    && pass "Omarchy's screensaver (needs ttfx, not in Arch) is put a day out" || fail "Omarchy's screensaver still runs at idle"
  [[ "$(st omarchy.polkit enabled)" == true ]] \
    && pass "Omarchy's polkit agent stays on" || fail "Omarchy's polkit agent is off"
  # friday.lock answers the lock IPC target that omarchy-system-lock, idle and
  # lid close call; a load error would leave the session with no lock screen.
  locked=$(timeout 10 qs -p "$W/omarchy/tree/shell" ipc call lock isLocked 2>/dev/null || true)
  [[ "$locked" == false ]] && pass "friday.lock answers the lock IPC target" \
    || fail "nothing answers the lock IPC target (got: ${locked:-nothing})"
  # The lock screen as it looks, without locking the test session.
  timeout 10 qs -p "$W/omarchy/tree/shell" ipc call lock preview >/dev/null 2>&1 && sleep 2 \
    && grim "$W/lock-preview.png" 2>/dev/null || true
  timeout 10 qs -p "$W/omarchy/tree/shell" ipc call lock hidePreview >/dev/null 2>&1 || true
  warns=$(sed 's/\x1b\[[0-9;]*m//g' "$W/omarchy.log" | grep -E "plugins/friday\.lock" | sort -u || true)
  if [[ -z "$warns" ]]; then pass "friday.lock loads without warnings"
  else fail "friday.lock logs warnings:"; sed 's/^/      /' <<<"$warns"; fi
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
