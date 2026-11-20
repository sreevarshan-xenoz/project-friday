#!/usr/bin/env bash
# The Linux hotkeys end to end, without a GPU. Super+Y, Super+Alt+Y and
# Super+Ctrl+Y (and their niri equivalents Mod+Y, Mod+Alt+Y, Mod+Ctrl+Y) as the
# shipped Hyprland and niri configs bind them, run the way each compositor runs
# a bind (sh -c for Hyprland, direct spawn argv for niri), through the real
# `friday` CLI, friday-ctl and daemon, with fake_model.py as the AI and the
# desktop tools stubbed: fuzzel "types" the question, wl-paste holds the
# selection, grim takes a PNG, notify-send records the notification. Requires,
# per key:
#   - every shipped config binds it to exactly its command and nothing else
#     (Super+Y once also opened ii's search overview),
#   - pressing it ends in a notification with the model's answer to that
#     question (for the screen, an answer about the screenshot it was sent).
# Neither compositor can run on a GPU-less runner (no GBM allocator), so the
# keymap comes from loading the configs with hypr_binds.lua and niri_binds.lua.
#
# Usage: tests/e2e/hotkeys.sh [bin-dir]   (default target/debug; needs lua, jq, python3)
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO=$PWD
BIN=$(cd "${1:-target/debug}" && pwd)
DAEMON_BIN="$BIN/friday-linux-daemon" CTL_BIN="$BIN/friday-ctl"
if [[ ! -x "$DAEMON_BIN" || ! -x "$CTL_BIN" ]]; then
  echo "no daemon/friday-ctl in $BIN (cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl)" >&2
  exit 2
fi
LUA=$(command -v lua5.4 || command -v lua || true)
for t in "$LUA" jq python3; do
  [[ -n "$t" ]] && command -v "$t" >/dev/null || { echo "needs lua, jq and python3" >&2; exit 2; }
done
A="$REPO/iso/friday/airootfs"
# FRIDAY OS's keymap is Omarchy's config plus ours; Omarchy (pinned) is
# fetched, not committed.
[[ -f "$A/usr/share/omarchy/default/hypr/bootstrap.lua" ]] || bash iso/friday/fetch-omarchy.sh

W=$(mktemp -d)
FAKE="" DAEMON=""
cleanup() { kill $DAEMON $FAKE 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT
fails=0
pass() { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; fails=$((fails + 1)); }

declare -A want=([SUPER+Y]="friday ask" [ALT+SUPER+Y]="friday ask --selection"
  [CTRL+SUPER+Y]="friday ask --screen")
keys=(SUPER+Y ALT+SUPER+Y CTRL+SUPER+Y)
declare -A niri_to_hypr=([Mod+Y]=SUPER+Y [Mod+Alt+Y]=ALT+SUPER+Y [Mod+Ctrl+Y]=CTRL+SUPER+Y)
niri_keys=(Mod+Y Mod+Alt+Y Mod+Ctrl+Y)

# keymap <label> <config entry> [home]: the effective binds, checked per key.
keymap() {
  local label=$1 entry=$2 home=${3:-$W} map="$W/keymap-${1//[^a-zA-Z]/}"
  if [[ "$entry" == *.kdl ]]; then
    if ! FRIDAY_SHARE="$REPO/packaging/linux" "$LUA" tests/e2e/niri_binds.lua "$entry" >"$map" 2>"$map.err"; then
      fail "$label: config did not load"; sed 's/^/       /' "$map.err" | head -5; return
    fi
    local k hk got
    for k in "${niri_keys[@]}"; do
      hk="${niri_to_hypr[$k]}"
      got=$(awk -F'\t' -v k="$k" '$1 == k { print $2 }' "$map")
      # niri's binds see only the press, so Mod+Y toggles listening (a plain
      # ask when voice is off) where Hyprland's Super+Y has press and release.
      local cmd="${want[$hk]}"; [[ "$k" == Mod+Y ]] && cmd="friday hold toggle"
      if [[ "$got" == "spawn $cmd" ]]; then pass "$label: $k runs \`$cmd\`"
      else fail "$label: $k should run only \`$cmd\`, runs:"; sed 's/^/       /' <<<"${got:-nothing}"; fi
    done
    return
  fi
  if ! HOME="$home" OMARCHY_PATH="$A/usr/share/omarchy" "$LUA" tests/e2e/hypr_binds.lua "$entry" >"$map" 2>"$map.err"; then
    fail "$label: config did not load"; sed 's/^/       /' "$map.err" | head -5; return
  fi
  local k got expect
  for k in "${keys[@]}"; do
    got=$(awk -F'\t' -v k="$k" '$1 == k && $2 !~ /^(release )?global quickshell:searchToggleReleaseInterrupt$/ { print $2 }' "$map")
    # Hyprland's Super+Y is a press and a release: a tap opens the prompt
    # (`friday ask`), a hold records speech (friday voice).
    expect="exec ${want[$k]}"
    [[ "$k" == SUPER+Y ]] && expect=$'exec friday hold start\nrelease exec friday hold end'
    if [[ "$got" == "$expect" ]]; then pass "$label: $k runs \`${expect//$'\n'/; }\`"
    else fail "$label: $k should run only \`${expect//$'\n'/; }\`, runs:"; sed 's/^/       /' <<<"${got:-nothing}"; fi
  done
}

echo "keymaps"
H="$W/home-omarchy"; mkdir -p "$H/.config"; cp -a "$A/etc/skel/.config/hypr" "$H/.config/hypr"
keymap "FRIDAY OS (Omarchy)" "$H/.config/hypr/hyprland.lua" "$H"
OMARCHY_MAP="$W/keymap-FRIDAYOSOmarchy"
H="$W/home-dots"; mkdir -p "$H/.config"
cp -a "$A/usr/share/friday/dots-hyprland/.config/hypr" "$H/.config/hypr"
keymap "Dots-Hyprland" "$H/.config/hypr/hyprland.lua" "$H"
keymap "friday-git friday.lua" "$REPO/packaging/linux/hyprland/friday.lua"
keymap "FRIDAY OS (niri)" "$A/etc/skel/.config/niri/config.kdl"
NIRI_MAP="$W/keymap-FRIDAYOSniri"
keymap "friday-git friday.kdl" "$REPO/packaging/linux/niri/friday.kdl"

echo "presses"
python3 tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
# Up to 90 s: Python can take a long time to start on a cold Windows runner
# (it has missed 30 s twice).
for _ in $(seq 900); do [[ -s "$W/port" ]] && break; kill -0 "$FAKE" 2>/dev/null || break; sleep 0.1; done
if [[ ! -s "$W/port" ]]; then
  if kill -0 "$FAKE" 2>/dev/null; then echo "the fake model was still starting after 90 s (no port file)" >&2
  else echo "the fake model exited before it had a port" >&2; cat "$W/model.log" >&2 2>/dev/null || true; fi
  exit 1
fi
PORT=$(cat "$W/port")

# Desktop stand-ins. The `friday` CLI itself is the packaged one.
S="$W/stub"; mkdir -p "$S" "$W/home"
ln -s "$REPO/packaging/linux/friday" "$S/friday"
printf '#!/bin/bash\necho "$PICK"\n' >"$S/fuzzel"
printf '#!/bin/bash\n[[ "$*" == *--primary* ]] && { printf %%s "$PRIMARY"; exit 0; }\nprintf %%s "$CLIP"\n' >"$S/wl-paste"
# notify-send: logs its arguments (tab-separated); -p prints an id, as libnotify does.
printf '#!/bin/bash\nprintf "%%s\\t" "$@" >>%q; echo >>%q\nfor a in "$@"; do [[ "$a" == -p ]] && echo 7; done; true\n' "$W/notify.log" "$W/notify.log" >"$S/notify-send"
# A real 1x1 PNG on stdout, as `grim -` writes.
printf '#!/bin/bash\necho "grim $*" >>%q\nbase64 -d <<<iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==\n' "$W/grim.log" >"$S/grim"
# hyprctl: the focused window a screen ask should capture (just it).
printf '#!/bin/bash\n[[ "$*" == "-j activewindow" ]] && echo %q\n' '{"class":"code","title":"main.rs","at":[100,50],"size":[800,600]}' >"$S/hyprctl"
chmod +x "$S"/*

offline=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  -u WAYLAND_DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE
  HOME="$W/home" XDG_STATE_HOME="$W/home/.local/state" PATH="$S:$BIN:$PATH"
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1"
  # The local model can see images, so a screen ask must stay on it (it
  # once always went to OpenCode Zen, whatever the user had configured).
  LLAMACPP_VISION=1)
"${offline[@]}" "$DAEMON_BIN" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
if grep -q '"status": *"running"' "$W/state.json" 2>/dev/null; then pass "daemon running"
else fail "daemon never reported running"; tail -20 "$W/daemon.log"; exit 1; fi

# press <key> <question> <expected answer (glob)> [env...]: run the key's
# command from the FRIDAY OS keymap as Hyprland or niri would, then wait for the
# answer notification.
press() {
  local key=$1 question=$2 expect=$3; shift 3
  local cmd runner=()
  if [[ "$key" == Mod+* ]]; then
    local line fields
    line=$(awk -F'\t' -v k="$key" '$1 == k && $2 ~ /^spawn / { print; exit }' "$NIRI_MAP")
    [[ -n "$line" ]] || { fail "$key: no command bound"; return; }
    IFS=$'\t' read -r -a fields <<<"$line"
    cmd="${fields[1]#spawn }"
    runner=("${fields[@]:2}")
  else
    cmd=$(awk -F'\t' -v k="$key" '$1 == k && $2 ~ /^exec / { sub(/^exec /, "", $2); print $2; exit }' "$OMARCHY_MAP")
    [[ -n "$cmd" ]] || { fail "$key: no command bound"; return; }
    # A quick tap: the press, then the release bind if there is one.
    local up; up=$(awk -F'\t' -v k="$key" '$1 == k && $2 ~ /^release exec / { sub(/^release exec /, "", $2); print $2; exit }' "$OMARCHY_MAP")
    [[ -z "$up" ]] || cmd="$cmd; $up"
    runner=(sh -c "$cmd")
  fi
  : >"$W/notify.log"
  "${offline[@]}" PICK="$question" "$@" timeout 150 setsid -w "${runner[@]}" </dev/null >>"$W/press.log" 2>&1 \
    || { fail "$key (\`$cmd\`) exited non-zero"; tail -5 "$W/press.log" | sed 's/^/       /'; return; }
  local line=""
  for _ in $(seq 50); do
    line=$(grep -F 'FRIDAY ✓' "$W/notify.log" || grep -F 'FRIDAY ✗' "$W/notify.log" || true)
    [[ -n "$line" ]] && break; sleep 0.2
  done
  # Notification args, tab-separated: ... <title> <body>; the body is last.
  local body; body=$(awk -F'\t' '{ print $(NF-1) }' <<<"$line" | tail -1)
  if [[ "$line" == *"FRIDAY ✓"* && "$body" == $expect ]]; then pass "$key: notified \"${body:0:80}\""
  else fail "$key: expected an answer notification like '$expect', got:"; sed 's/^/       /' "$W/notify.log"; fi
  # Every ask streams, the screen too: the notification fills in, then the
  # final answer replaces that same notification (-r with the id -p gave).
  local updates; updates=$(grep -cE $'^-a\tFRIDAY\t-r\t7\t(--\t)?FRIDAY · ' "$W/notify.log" || true)
  if (( updates >= 1 )) && [[ "$line" == *$'-r\t7\t'* ]]; then pass "$key: the answer filled in ($updates updates), then replaced them"
  else fail "$key: expected streaming updates and a final replace (updates: $updates)"; sed 's/^/       /' "$W/notify.log"; fi
}

press SUPER+Y "What does the Rust error E0308 mean?" "FAKE-MODEL-ANSWER: What does the Rust error E0308 mean?*"
press Mod+Y "What does the Rust error E0308 mean in niri?" "FAKE-MODEL-ANSWER: What does the Rust error E0308 mean in niri?*"
press ALT+SUPER+Y "explain this" "FAKE-MODEL-ANSWER: explain this*" \
  PRIMARY="error[E0308]: mismatched types: expected u32, found String"
if grep -q "mismatched types" "$W/model.log"; then pass "ALT+SUPER+Y: the selection reached the model"
else fail "ALT+SUPER+Y: the selected text never reached the model"; fi
press CTRL+SUPER+Y "what is on my screen?" "FAKE-MODEL-ANSWER: what is on my screen? \[saw a [1-9]*-byte 1x1 image/png\]"
if grep -qx 'grim -g 100,50 800x600 -' "$W/grim.log"; then pass "CTRL+SUPER+Y: captured just the focused window (grim -g 100,50 800x600)"
else fail "CTRL+SUPER+Y: expected grim to capture the focused window, ran:"; sed 's/^/       /' "$W/grim.log"; fi

press Mod+Y "What does the Rust error E0308 mean?" "FAKE-MODEL-ANSWER: What does the Rust error E0308 mean?*"
press Mod+Alt+Y "explain this" "FAKE-MODEL-ANSWER: explain this*" \
  PRIMARY="error[E0308]: mismatched types in niri: expected u32, found String"
if grep -q "mismatched types in niri" "$W/model.log"; then pass "Mod+Alt+Y: the selection reached the model"
else fail "Mod+Alt+Y: the selected text never reached the model"; fi
: >"$W/grim.log"
# The window crop isn't checked here: the stubbed hyprctl answers on niri too,
# and real niri has no hyprctl, so the crop needs its own niri test.
press Mod+Ctrl+Y "what is on my screen?" "FAKE-MODEL-ANSWER: what is on my screen? \[saw a [1-9]*-byte 1x1 image/png\]"

if grep -q "panicked" "$W/daemon.log"; then fail "daemon panicked:"; grep -A1 panicked "$W/daemon.log" | head -4
else pass "no panics"; fi

# The same press when the user's only provider can't see images: refused,
# and the screenshot goes nowhere (never to keyless OpenCode Zen instead).
echo "screen ask without a vision model"
kill $DAEMON 2>/dev/null; wait $DAEMON 2>/dev/null || true
rm -f "$W/state.json"
offline=("${offline[@]/LLAMACPP_VISION=1/LLAMACPP_VISION=0}")
"${offline[@]}" "$DAEMON_BIN" >"$W/daemon2.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
lines_before=$(wc -l <"$W/model.log")
: >"$W/notify.log"
"${offline[@]}" PICK="what is on my screen?" timeout 150 setsid -w sh -c "friday ask --screen" </dev/null >>"$W/press.log" 2>&1 || true
for _ in $(seq 50); do grep -qF 'FRIDAY ✗' "$W/notify.log" && break; sleep 0.2; done
if grep -F 'FRIDAY ✗' "$W/notify.log" | grep -q "not sent anywhere"; then pass "refused: \"$(grep -oE 'None of your[^.]*\.' "$W/notify.log" | head -1)\""
else fail "expected a refusal saying the screenshot was not sent:"; sed 's/^/       /' "$W/notify.log"; fi
if [[ $(wc -l <"$W/model.log") -eq $lines_before ]]; then pass "no model was sent anything"
else fail "a model was called for the refused screen ask"; tail -n +$((lines_before + 1)) "$W/model.log" | cut -c1-200; fi
if grep -q "opencode.ai/zen/v1/chat" "$W/daemon2.log"; then fail "the daemon tried OpenCode Zen for the screen ask"
else pass "OpenCode Zen was never tried"; fi

if (( fails > 0 )); then echo "e2e hotkeys: $fails failure(s) (logs in $W)"; exit 1; fi
echo "e2e hotkeys: ok"
