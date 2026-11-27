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
# A camera is a file here (the tour offers face unlock when one exists).
export FRIDAY_VIDEO_DEVS="$W/video0" FRIDAY_FACE_PAM="$W/no-face-pam"
# tourup: wait (up to 5 s) for the tour to show, once its checks are done.
tourup() { local i; for i in $(seq 25); do [[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 0 ]] && return 0; sleep 0.2; done; return 1; }
# A stand-in friday for the shell: the app-bundles switch is on while
# $W/bundles-on exists (the tour then offers app bundles).
mkdir -p "$W/bin"
printf '#!/bin/sh\n[ "$*" = "feature app-bundles status" ] && { [ -e "%s/bundles-on" ] && echo "app-bundles: on" || echo "app-bundles: off"; }\nexit 0\n' "$W" >"$W/bin/friday"
chmod +x "$W/bin/friday"; export PATH="$W/bin:$PATH"
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
# sway has it, so its active workspace shows as a core-red pill (the default palette) by the launcher.
if [[ -s "$W/friday-bar.png" ]] && need python3; then
  n=$(python3 "$(dirname "$0")/png_colour.py" "$W/friday-bar.png" "#d42d51" "36,6,120,30" 2>/dev/null || echo 0)
  if (( n > 40 )); then pass "workspaces show off Hyprland (ext-workspace, as on niri)"
  else fail "no workspace pill off Hyprland ($n core-red px by the launcher)"; fi
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
  [[ "$(timeout 10 qs -p "$FS" ipc call answer codeCount 2>/dev/null)" == 0 ]] \
    && pass "an answer with no code has no Copy code" || fail "Copy code shown with no code"
  timeout 10 qs -p "$FS" ipc call answer present true "Clean the cache" $'Run:\n\n```bash\nsudo paccache -r\n```\n\nThen:\n\n```\nfriday doctor\n```' "" "" "" >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer codeCount 2>/dev/null)" == 2 ]] \
    && pass "each code block in an answer gets its own Copy" || fail "code blocks not found: $(timeout 10 qs -p "$FS" ipc call answer codeCount 2>/dev/null)"
  timeout 10 qs -p "$FS" ipc call answer listening >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == listening ]] \
    && pass "answer pill shows Listening while Super+Y is held" || fail "answer pill has no listening state"
  timeout 10 qs -p "$FS" ipc call answer dictating >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == listening ]] \
    && pass "answer pill shows Listening while Super+Shift+Y (voice typing) is held" || fail "answer pill has no dictating state"
  timeout 10 qs -p "$FS" ipc call answer receipt "what is this?" "nautilus · a screenshot of the window" >/dev/null 2>&1 || true
  [[ "$(timeout 10 qs -p "$FS" ipc call answer state 2>/dev/null)" == working ]] \
    && pass "answer pill shows what was sent while it works" || fail "the pill didn't take the capture receipt"
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
tourup || true
for _ in 1 2 3 4 5; do timeout 10 qs -p "$FS" ipc call tour next >/dev/null 2>&1 || true; done
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 5 ]] && pass "tour has a phone step when Pocketdesk is installed" || fail "tour has no phone step"
sleep 1; grim "$W/friday-tour-phone.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call tour back >/dev/null 2>&1 || true
[[ "$(timeout 10 qs -p "$FS" ipc call tour step 2>/dev/null)" == 4 ]] && pass "tour can go back a step" || fail "tour did not go back"
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
# With a camera the tour offers face unlock, one step more.
: >"$W/video0"
timeout 10 qs -p "$FS" ipc call tour begin "Alex" true false >/dev/null 2>&1 || true
tourup || fail "the tour didn't show with a camera"
[[ "$(timeout 10 qs -p "$FS" ipc call tour count 2>/dev/null)" == 6 ]] && pass "with a camera the tour offers face unlock" || fail "no face unlock step with a camera"
for _ in 1 2 3 4; do timeout 10 qs -p "$FS" ipc call tour next >/dev/null 2>&1 || true; done
sleep 1; grim "$W/friday-tour-face.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
rm -f "$W/video0"
timeout 10 qs -p "$FS" ipc call tour begin "Alex" true false >/dev/null 2>&1 || true
tourup || fail "the tour didn't show without a camera"
[[ "$(timeout 10 qs -p "$FS" ipc call tour count 2>/dev/null)" == 5 ]] && pass "without one it doesn't" || fail "a face unlock step without a camera"
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
# The app bundles step, unless switched off.
: >"$W/bundles-on"
timeout 10 qs -p "$FS" ipc call tour begin "Alex" true false >/dev/null 2>&1 || true
tourup || fail "the tour didn't show with app bundles on"
[[ "$(timeout 10 qs -p "$FS" ipc call tour count 2>/dev/null)" == 6 ]] && pass "the tour offers app bundles" || fail "no app bundles step"
for _ in 1 2 3 4; do timeout 10 qs -p "$FS" ipc call tour next >/dev/null 2>&1 || true; done
sleep 1; grim "$W/friday-tour-bundles.png" 2>/dev/null || true
timeout 10 qs -p "$FS" ipc call tour finish >/dev/null 2>&1 || true
rm -f "$W/bundles-on"
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
# The launcher's quick commands: found by what a person types, nothing for a stray letter.
[[ "$(timeout 10 qs -p "$FS" ipc call commands match mut 2>/dev/null)" == "Mute|Unmute" ]] \
  && pass "launcher finds Mute and Unmute for 'mut'" || fail "launcher commands for 'mut': $(timeout 10 qs -p "$FS" ipc call commands match mut 2>/dev/null)"
[[ "$(timeout 10 qs -p "$FS" ipc call commands match dnd 2>/dev/null)" == "Do not disturb: on|Do not disturb: off" ]] \
  && pass "launcher finds do not disturb by 'dnd'" || fail "launcher commands for 'dnd' wrong"
[[ -z "$(timeout 10 qs -p "$FS" ipc call commands match a 2>/dev/null)" && -z "$(timeout 10 qs -p "$FS" ipc call commands match zzzzq 2>/dev/null)" ]] \
  && pass "launcher lists no command for one letter or nonsense" || fail "launcher listed commands for one letter or nonsense"
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
# The bar's media chip keeps a paused player so it can be resumed (a stand-in
# MPRIS player on the session bus; needs the check to run under dbus-run-session).
if [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && python3 -c 'import dbus, gi' 2>/dev/null; then
  python3 "$(dirname "$0")/fake-mpris.py" Paused & MPRIS=$!
  sleep 2
  [[ "$(timeout 10 qs -p "$FS" ipc call media state 2>/dev/null)" == paused ]] \
    && pass "the media chip keeps a paused player" || fail "a paused player vanished from the media chip: $(timeout 10 qs -p "$FS" ipc call media state 2>/dev/null)"
  kill -USR1 "$MPRIS"; sleep 1
  [[ "$(timeout 10 qs -p "$FS" ipc call media state 2>/dev/null)" == playing ]] \
    && pass "the media chip shows a playing player" || fail "the media chip missed playback resuming"
  kill "$MPRIS" 2>/dev/null; wait "$MPRIS" 2>/dev/null; sleep 1.5
  [[ "$(timeout 10 qs -p "$FS" ipc call media state 2>/dev/null)" == none ]] \
    && pass "the media chip goes away with the player" || fail "the media chip stayed after the player quit"
else skip "media chip (needs dbus-python, PyGObject and dbus-run-session)"; fi
# Every style with a palette, switched live through shell.json as the Look
# picker does: the bar, the FRIDAY drawer and the control drawer each time.
mkdir -p "$HOME/.config/friday"
# The bar must really show the look: enough of the palette's background in
# the half of the screen the style puts the bar in (png_colour.py). The bar
# is frosted glass (Theme.glass: the background at 72%, 80% for the light
# dawn) over the headless output's black, with no blur here.
declare -A look_bg=([core]=0c0e11 [ember]=090b0e [polar]=21252e [frost]=0a0e13 [moss]=0b0d0a [dawn]=c6c3be)
for look in lean:core island:polar dock:frost lean:moss ember:dawn island:dawn dock:ember rail:core rail:dawn keystone:core keystone:dawn; do
  style=${look%%:*} palette=${look##*:}
  # The dock and the shelf are on for these, so they load under every style.
  printf '{ "style": "%s", "palette": "%s", "dock": true, "dockApps": ["foot"], "shelf": true }\n' "$style" "$palette" >"$HOME/.config/friday/shell.json"
  sleep 3; grim "$W/look-${look/:/-}-bar.png" 2>/dev/null || true
  half=top; [[ $style == dock ]] && half=bottom; [[ $style == rail ]] && half="0,0,46,900"
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
