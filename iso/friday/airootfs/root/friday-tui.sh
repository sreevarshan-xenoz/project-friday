# shellcheck shell=bash
# Shared look & feel for the FRIDAY live installer (configurator + installer).
# Sourced, not executed.

FRIDAY_ACCENT="#7AA2F7"      # Deep Ice Blue
FRIDAY_SECONDARY="#BB9AF7"   # Soft Purple
FRIDAY_MUTED="#565F89"       # Muted Slate
FRIDAY_SUCCESS="#9ECE6A"     # Mint Green
FRIDAY_WARNING="#E0AF68"     # Warm Amber
FRIDAY_ERROR="#F7768E"       # Crimson Red
FRIDAY_BORDER="#3B4261"      # Border Slate
FRIDAY_TEXT="#C0CAF5"        # Soft White
FRIDAY_BG="#1A1B26"          # Night

FRIDAY_CYAN="#7DCFFF"        # Sky
FRIDAY_SOFT="#9AA5CE"        # Soft Slate (readable muted text)

# The Linux console (where setup runs) has 16 colour slots, so truecolor would
# be rounded to the stock VGA colours. Instead each FRIDAY colour gets a slot,
# the slot is redefined to that exact colour (friday_console_theme), and gum
# is given slot numbers. Slot 0 is the background, so the screen is Night.
FRIDAY_CONSOLE_PALETTE=(
  "$FRIDAY_BG" "$FRIDAY_ERROR" "$FRIDAY_SUCCESS" "$FRIDAY_WARNING"
  "$FRIDAY_ACCENT" "$FRIDAY_SECONDARY" "$FRIDAY_CYAN" "$FRIDAY_TEXT"
  "#626A8E" "$FRIDAY_ERROR" "$FRIDAY_SUCCESS" "$FRIDAY_WARNING"
  "#A9C1FF" "$FRIDAY_SECONDARY" "$FRIDAY_SOFT" "#FFFFFF"
)
if [[ "${TERM:-}" == linux ]]; then
  FRIDAY_BG=0 FRIDAY_ERROR=1 FRIDAY_SUCCESS=2 FRIDAY_WARNING=3 FRIDAY_ACCENT=4
  FRIDAY_SECONDARY=5 FRIDAY_CYAN=6 FRIDAY_TEXT=7 FRIDAY_BORDER=8 FRIDAY_SOFT=14
  FRIDAY_MUTED=14
fi
# Glyphs the console's Terminus font lacks (✓ ✗, rounded corners) have
# stand-ins there.
FRIDAY_OK_GLYPH="✓" FRIDAY_ERR_GLYPH="✗" FRIDAY_BORDER_STYLE=rounded FRIDAY_CURSOR="❯ "
if [[ "${TERM:-}" == linux ]]; then
  FRIDAY_OK_GLYPH="■" FRIDAY_ERR_GLYPH="×" FRIDAY_BORDER_STYLE=normal FRIDAY_CURSOR="> "
fi

# On the Linux console: the palette above and a Terminus font sized so the
# screen is about 100 columns wide. A no-op anywhere else.
friday_console_theme() {
  [[ "${TERM:-}" == linux && -t 1 ]] || return 0
  local i hex
  for i in "${!FRIDAY_CONSOLE_PALETTE[@]}"; do
    hex="${FRIDAY_CONSOLE_PALETTE[$i]#\#}"
    printf '\e]P%X%s' "$i" "$hex"
  done
  local width font
  width=$(cut -d, -f1 /sys/class/graphics/fb0/virtual_size 2>/dev/null || echo 0)
  if (( width >= 2400 )); then font=ter-v32b
  elif (( width >= 1800 )); then font=ter-v28b
  elif (( width >= 1200 )); then font=ter-v24b
  elif (( width >= 900 )); then font=ter-v20b
  else font=ter-v16b; fi
  setfont "$font" 2>/dev/null || true
  printf '\e[0m'
  clear 2>/dev/null || true
  friday_layout
}

# Content sits in a centred column about 96 characters wide: FRIDAY_INDENT is
# its left edge, given to every gum prompt and to FRIDAY's own lines.
friday_layout() {
  local cols
  cols=$(tput cols 2>/dev/null || echo 80)
  FRIDAY_INDENT=$(( cols > 100 ? (cols - 96) / 2 : 2 ))
  FRIDAY_PAD=$(printf '%*s' "$FRIDAY_INDENT" '')
  FRIDAY_WIDTH=$(( cols > 100 ? 96 : cols - 4 ))
  local pad="0 0 0 $FRIDAY_INDENT"
  export GUM_CHOOSE_PADDING="$pad" GUM_CONFIRM_PADDING="$pad" GUM_INPUT_PADDING="$pad" \
    GUM_FILTER_PADDING="$pad" GUM_SPIN_PADDING="$pad" GUM_WRITE_PADDING="$pad"
}
friday_layout

# Put every gum prompt in FRIDAY's palette (gum defaults to pink).
export GUM_CHOOSE_CURSOR_FOREGROUND="$FRIDAY_ACCENT"
export GUM_CHOOSE_SELECTED_FOREGROUND="$FRIDAY_ACCENT"
export GUM_CHOOSE_HEADER_FOREGROUND="$FRIDAY_SECONDARY"
export GUM_CHOOSE_ITEM_FOREGROUND="$FRIDAY_TEXT"
export GUM_CONFIRM_PROMPT_FOREGROUND="$FRIDAY_SECONDARY"
export GUM_CONFIRM_SELECTED_BACKGROUND="$FRIDAY_ACCENT"
export GUM_CONFIRM_SELECTED_FOREGROUND="$FRIDAY_BG"
export GUM_CONFIRM_UNSELECTED_BACKGROUND="$FRIDAY_BORDER"
export GUM_CONFIRM_UNSELECTED_FOREGROUND="$FRIDAY_TEXT"
export GUM_INPUT_PROMPT_FOREGROUND="$FRIDAY_SECONDARY"
export GUM_INPUT_CURSOR_FOREGROUND="$FRIDAY_ACCENT"
export GUM_INPUT_PLACEHOLDER_FOREGROUND="$FRIDAY_MUTED"
export GUM_FILTER_INDICATOR_FOREGROUND="$FRIDAY_ACCENT"
export GUM_FILTER_MATCH_FOREGROUND="$FRIDAY_ACCENT"
export GUM_FILTER_PROMPT_FOREGROUND="$FRIDAY_SECONDARY"
export GUM_FILTER_PLACEHOLDER_FOREGROUND="$FRIDAY_MUTED"
export GUM_FILTER_HEADER_FOREGROUND="$FRIDAY_SECONDARY"
export GUM_SPIN_SPINNER_FOREGROUND="$FRIDAY_ACCENT"
export GUM_SPIN_TITLE_FOREGROUND="$FRIDAY_TEXT"
export GUM_CHOOSE_HEADER=""

# FRIDAY_FAST=1 (or a non-tty) disables the typewriter effect.
friday_is_tty() { [[ -t 1 && "${FRIDAY_FAST:-0}" != "1" ]]; }

friday_banner() {
  local subtitle="${1:-COGNITIVE OPERATING SYSTEM}"
  local cols
  cols=$(tput cols 2>/dev/null || echo 80)
  # Unattended runs (SSH, no TERM) have no screen to clear; never fail on it.
  clear 2>/dev/null || true
  echo
  gum style --width "$cols" --foreground "$FRIDAY_ACCENT" --align center \
"██████████  ████████    ██████  ████████      ██████    ██      ██" \
"██          ██      ██    ██    ██      ██  ██      ██  ██      ██" \
"████████    ████████      ██    ██      ██  ██████████    ██████  " \
"██          ██    ██      ██    ██      ██  ██      ██      ██    " \
"██          ██      ██  ██████  ████████    ██      ██      ██    "
  echo
  gum style --width "$cols" --foreground "$FRIDAY_SECONDARY" --bold --align center "$subtitle"
  gum style --width "$cols" --foreground "$FRIDAY_MUTED" --align center "────────────────────────────────────────────────────────────"
  echo
}

# The escape for a foreground colour: a console slot number or a #rrggbb.
friday_fg() {
  case "$1" in
    \#*) printf '\e[38;2;%d;%d;%dm' "0x${1:1:2}" "0x${1:3:2}" "0x${1:5:2}" ;;
    [0-7]) printf '\e[3%dm' "$1" ;;
    *) printf '\e[9%dm' "$(( $1 - 8 ))" ;;
  esac
}

friday_bg() {
  case "$1" in
    \#*) printf '\e[48;2;%d;%d;%dm' "0x${1:1:2}" "0x${1:3:2}" "0x${1:5:2}" ;;
    [0-7]) printf '\e[4%dm' "$1" ;;
    *) printf '\e[10%dm' "$(( $1 - 8 ))" ;;
  esac
}

# A compact header for each step (the big wordmark is for the welcome and
# finish screens): "FRIDAY · <subtitle>" over a rule, centred.
friday_header() {
  local subtitle="${1:-SETUP}" cols
  cols=$(tput cols 2>/dev/null || echo 80)
  clear 2>/dev/null || true
  echo
  local text="FRIDAY  ·  $subtitle" pad
  pad=$(( (cols - ${#text}) / 2 )); (( pad < 0 )) && pad=0
  printf '%*s\e[1m%sFRIDAY\e[0m  %s·  %s\e[0m\n' "$pad" '' "$(friday_fg "$FRIDAY_ACCENT")" \
    "$(friday_fg "$FRIDAY_MUTED")" "$(friday_fg "$FRIDAY_SECONDARY")$subtitle"
  local rule_w=$(( cols > 100 ? 96 : cols - 4 ))
  printf '%*s%s%s\e[0m\n\n' "$(( (cols - rule_w) / 2 ))" '' "$(friday_fg "$FRIDAY_BORDER")" \
    "$(printf '─%.0s' $(seq 1 "$rule_w"))"
}

# Spoken lines: only once the person has asked for the voice (step_network
# downloads it into RAM and creates the `on` marker). A new line replaces the
# one being said.
FRIDAY_LIVE_VOICE=/run/friday-voice
friday_speak() {
  [[ -e "$FRIDAY_LIVE_VOICE/on" ]] || return 0
  FRIDAY_VOICE_DIR="$FRIDAY_LIVE_VOICE" friday-ctl say "$*" >/dev/null 2>&1 &
}

# FRIDAY "speaks": a soft purple line typed out character by character.
friday_say() {
  local text="$*"
  friday_speak "$text"
  if ! friday_is_tty; then
    printf '%sFRIDAY › %s\n' "$FRIDAY_PAD" "$text"
    return
  fi
  printf '%s%sFRIDAY ›\e[0m %s' "$FRIDAY_PAD" "$(friday_fg "$FRIDAY_SECONDARY")" "$(friday_fg "$FRIDAY_TEXT")"
  local i ch
  for ((i = 0; i < ${#text}; i++)); do
    ch="${text:i:1}"
    printf '%s' "$ch"
    case "$ch" in
      [.?!]) sleep 0.18 ;;
      ,) sleep 0.08 ;;
      *) sleep 0.012 ;;
    esac
  done
  printf '\e[0m\n'
}

friday_ok()   { gum style --margin "0 0 0 $FRIDAY_INDENT" --foreground "$FRIDAY_SUCCESS" "$FRIDAY_OK_GLYPH $*"; }
friday_warn() { gum style --margin "0 0 0 $FRIDAY_INDENT" --foreground "$FRIDAY_WARNING" "! $*"; }
friday_err()  { gum style --margin "0 0 0 $FRIDAY_INDENT" --foreground "$FRIDAY_ERROR" "$FRIDAY_ERR_GLYPH $*"; }
friday_head() { gum style --margin "0 0 0 $FRIDAY_INDENT" --foreground "$FRIDAY_ACCENT" --bold "$*"; }
friday_note() { gum style --margin "0 0 0 $FRIDAY_INDENT" --width "$FRIDAY_WIDTH" --foreground "$FRIDAY_MUTED" "$*"; }

# Progress rail: friday_rail <current-index> <label>...
# Done steps are green squares, the current one an ice-blue pill, the rest
# muted dots (glyphs the console's Terminus font has), centred.
friday_rail() {
  local current="$1"; shift
  local cols; cols=$(tput cols 2>/dev/null || echo 80)
  local labels=("$@") total=0 label
  for label in "${labels[@]}"; do total=$((total + ${#label} + 5)); done
  # Too wide for the screen: only the current step keeps its name, the rest
  # show their number, so the rail never wraps onto a second line.
  local short=false
  (( total > cols - 4 )) && short=true
  local out="" plain="" i=1 shown
  for label in "${labels[@]}"; do
    shown="$label"
    if $short && (( i != current )); then shown="$i"; fi
    if (( i < current )); then
      out+="$(friday_fg "$FRIDAY_SUCCESS")■ $shown\e[0m   "; plain+="■ $shown   "
    elif (( i == current )); then
      # No bold: the console shows bold as the bright colour, a light label.
      out+="$(friday_bg "$FRIDAY_ACCENT")$(friday_fg "$FRIDAY_BG") $shown \e[0m   "; plain+=" $shown    "
    else
      out+="$(friday_fg "$FRIDAY_MUTED")• $shown\e[0m   "; plain+="• $shown   "
    fi
    i=$((i + 1))
  done
  local pad
  pad=$(( (cols - ${#plain} + 3) / 2 )); (( pad < 0 )) && pad=0
  printf '%*s%b\n\n' "$pad" '' "${out%   }"
}

# What the person lets FRIDAY do (the FEATURES step). Everything defaults to
# what an unattended install gets: all on except sending a window's text.
# (The phone is only offered at first login; Pocketdesk stays off until a yes.)
FEAT_DESKTOP="${FEAT_DESKTOP:-true}"       # control windows, apps, volume, media
FEAT_TASKS="${FEAT_TASKS:-true}"           # standing tasks ("every morning at 8 ...")
FEAT_APPNAME="${FEAT_APPNAME:-true}"       # name the app a selection came from
FEAT_SCREENTEXT="${FEAT_SCREENTEXT:-false}" # send a window's text, not a screenshot
FEAT_PHONE="${FEAT_PHONE:-true}"            # offer to pair a phone (Pocketdesk) at first login

# friday_write_features <config-dir>: the chosen settings, in the files the
# `friday` CLI and the daemon already read. Only what differs from the
# defaults is written, so a default install changes nothing.
friday_write_features() {
  local dir="$1" env="$1/friday.env"
  mkdir -p "$dir"
  (
    umask 077
    touch "$env"
    [[ "$FEAT_DESKTOP" == true ]] || echo "FRIDAY_DESKTOP=0" >>"$env"
    [[ "$FEAT_TASKS" == true ]] || echo "FRIDAY_STANDING_TASKS=0" >>"$env"
    [[ "$FEAT_SCREENTEXT" != true ]] || echo "FRIDAY_SCREEN_TEXT=1" >>"$env"
    [[ -s "$env" ]] || rm -f "$env"
  )
  [[ "$FEAT_APPNAME" == true ]] || : >"$dir/no-app-context"
  [[ "$FEAT_PHONE" == true ]] || : >"$dir/no-phone-offer"
}

# --- validators (pure; unit-testable) -------------------------------------

# RFC 1123 label, lowercase.
friday_valid_hostname() { [[ "$1" =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$ ]]; }

# useradd-safe login name, not a system account.
friday_valid_username() {
  [[ "$1" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || return 1
  case "$1" in
    root|bin|daemon|mail|ftp|http|nobody|dbus|systemd-*|polkitd|rtkit|sddm|git|avahi) return 1 ;;
  esac
}

friday_valid_timezone() {
  [[ -n "$1" && "$1" != *..* && -f "/usr/share/zoneinfo/$1" ]]
}

# Written verbatim into a systemd EnvironmentFile; nothing that needs quoting.
friday_valid_api_key() { [[ "$1" =~ ^[A-Za-z0-9._:-]{8,}$ ]]; }

friday_online() {
  curl -fsI --max-time 6 https://geo.mirror.pkgbuild.com/ >/dev/null 2>&1 \
    || curl -fsI --max-time 6 https://archlinux.org/ >/dev/null 2>&1
}

# Pick lists that never come back empty (an empty stdin makes `gum filter`
# list files in the current directory instead).
friday_timezones() {
  timedatectl list-timezones 2>/dev/null | grep . \
    || awk '!/^#/ {print $3}' /usr/share/zoneinfo/zone1970.tab 2>/dev/null | sort -u \
    || echo UTC
}
friday_keymaps() {
  localectl list-keymaps 2>/dev/null | grep . \
    || find /usr/share/kbd/keymaps -name '*.map.gz' -printf '%f\n' 2>/dev/null | sed 's/\.map\.gz$//' | sort -u | grep . \
    || echo us
}
friday_valid_keymap() { [[ -n "$1" ]] && friday_keymaps | grep -qxF -- "$1"; }

# The desktop (XKB) layout for a console keymap, as systemd maps them
# (localectl does the same): prints "layout model variant options", "-" for
# none. Tries the keymap, then its base name (de-latin1-nodeadkeys -> de);
# a keymap that is itself an XKB layout name stands for that layout; else us.
#   friday_xkb_for_keymap <keymap> [kbd-model-map] [xkb base.lst]
friday_xkb_for_keymap() {
  local km=$1 map=${2:-/usr/share/systemd/kbd-model-map} lst=${3:-/usr/share/X11/xkb/rules/base.lst}
  local try line
  for try in "$km" "${km%%-*}"; do
    line=$(awk -v k="$try" '!/^#/ && $1 == k { print $2, $3, $4, $5; exit }' "$map" 2>/dev/null)
    if [[ -n "$line" ]]; then echo "$line"; return; fi
  done
  try=${km%%-*}
  if [[ -n "$try" ]] && awk '/^! layout/ {on = 1; next} /^!/ {on = 0} on {print $1}' "$lst" 2>/dev/null | grep -qxF -- "$try"; then
    echo "$try pc105 - -"; return
  fi
  echo "us pc105 - -"
}

# Add the LUKS unlock hook to an mkinitcpio.conf before `filesystems` and print
# the kernel parameters that go with it. systemd-based initramfs (current Arch
# default) uses sd-encrypt + rd.luks.name; busybox-based uses encrypt +
# cryptdevice. Idempotent.
#   friday_add_crypt_hook <mkinitcpio.conf> <luks-uuid> <mapper-name>
friday_add_crypt_hook() {
  local conf="$1" uuid="$2" name="$3" hook params
  if grep -qE '^HOOKS=\(.*\bsystemd\b' "$conf"; then
    hook=sd-encrypt params="rd.luks.name=$uuid=$name"
  else
    hook=encrypt params="cryptdevice=UUID=$uuid:$name"
  fi
  if ! grep -qE "^HOOKS=\(.*\b$hook\b" "$conf"; then
    sed -i -E "/^HOOKS=\(/ s/\bfilesystems\b/$hook filesystems/" "$conf"
  fi
  grep -qE "^HOOKS=\(.*\b$hook filesystems\b" "$conf" || return 1
  printf '%s root=/dev/mapper/%s\n' "$params" "$name"
}
