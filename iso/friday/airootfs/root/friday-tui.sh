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

# friday_orb <frame>: FRIDAY's orb in plain ASCII (the console font has
# nothing fancier), its rings of light moving outward frame by frame. Drawn
# under the live status line while a long step waits.
FRIDAY_ORB_FRAMES=4
friday_orb() {
  local k=$(( $1 % FRIDAY_ORB_FRAMES )) a=" " b=" " c=" "
  # The wave: the inner ring lights, then the middle, then the outer.
  case $k in 0) a="*" ;; 1) a="." b="*" ;; 2) b="." c="*" ;; 3) c="." ;; esac
  printf '%s\n' \
    "            $c       $c" \
    "        $c     $b   $b     $c" \
    "      $c    $b   $a $a $a   $b    $c" \
    "     $c   $b   $a (@) $a   $b   $c" \
    "      $c    $b   $a $a $a   $b    $c" \
    "        $c     $b   $b     $c" \
    "            $c       $c"
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
# (The phone is only offered at first login; FRIDAY Mobile stays off until a yes.)
FEAT_DESKTOP="${FEAT_DESKTOP:-true}"       # control windows, apps, volume, media
FEAT_TASKS="${FEAT_TASKS:-true}"           # standing tasks ("every morning at 8 ...")
FEAT_APPNAME="${FEAT_APPNAME:-true}"       # name the app a selection came from
FEAT_SCREENTEXT="${FEAT_SCREENTEXT:-true}"  # send a window's text, not a screenshot
FEAT_PHONE="${FEAT_PHONE:-true}"            # offer to pair a phone (FRIDAY Mobile) at first login

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
    [[ "$FEAT_SCREENTEXT" == true ]] || echo "FRIDAY_SCREEN_TEXT=0" >>"$env"
    [[ -s "$env" ]] || rm -f "$env"
  )
  [[ "$FEAT_APPNAME" == true ]] || : >"$dir/no-app-context"
  [[ "$FEAT_PHONE" == true ]] || : >"$dir/no-phone-offer"
}

# --- validators (pure; unit-testable) -------------------------------------

# The disk the live system booted from (/dev/sdX), or nothing: the parent of
# the partition mounted at bootmnt (or that device itself, for a CD), resolved
# through any /dev/disk/by-* link.
friday_live_disk() {
  local src dev
  src=$(findmnt -n -o SOURCE /run/archiso/bootmnt 2>/dev/null) && [[ -n "$src" ]] || return 0
  src=$(readlink -f "$src")
  dev=$(lsblk -n -d -o PKNAME "$src" 2>/dev/null | head -n1)
  if [[ -n "$dev" ]]; then echo "/dev/$dev"; else echo "$src"; fi
}

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

# ------------------------------------------------------------ dual boot
# "Install alongside": FRIDAY goes into free space that already exists on a
# GPT disk with an EFI partition. Existing partitions are never moved,
# resized or formatted; the EFI partition only gains systemd-boot.

FRIDAY_MIN_MB=40960        # 40 GB: the system, updates and a little room
FRIDAY_ESP_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b
FRIDAY_XBOOTLDR_TYPE=bc13c2ff-59e6-4262-a352-b275fd6f7172
FRIDAY_ROOT_TYPE=4f68bce3-e8cd-4db1-96e7-fbcaf984b709

# The partition device for number N of a disk (nvme0n1 → nvme0n1p3).
friday_part_dev() { if [[ "$1" =~ [0-9]$ ]]; then echo "${1}p$2"; else echo "${1}$2"; fi; }

# What's on a disk, as key=value lines: table (gpt/dos/empty), parts (how
# many partitions), free_mb (the largest free block), esp (its device),
# esp_free_mb, windows (1 when Windows Boot Manager is on the ESP), others
# (other systems' folders on the ESP), bitlocker (1 when a partition is
# BitLocker-encrypted). Read-only: the ESP is mounted read-only to look.
friday_disk_scan() {
  local disk="$1" ss first last esp mnt d others=""
  echo "table=$(lsblk -n -d -o PTTYPE "$disk" 2>/dev/null | xargs)"
  # (|| true: grep -c fails on no match, which the installer's ERR trap catches.)
  echo "parts=$(lsblk -l -n -o TYPE "$disk" 2>/dev/null | grep -c '^part$' || true)"
  ss=$(blockdev --getss "$disk" 2>/dev/null || echo 512)
  first=$(sgdisk -F "$disk" 2>/dev/null | tail -n1)
  last=$(sgdisk -E "$disk" 2>/dev/null | tail -n1)
  if [[ "$first" =~ ^[0-9]+$ && "$last" =~ ^[0-9]+$ ]] && (( last > first )); then
    echo "free_mb=$(( (last - first + 1) * ss / 1048576 ))"
  else
    echo "free_mb=0"
  fi
  # Every EFI partition on the disk: the one with Windows Boot Manager if
  # there is one (OEM tools or an older Linux may have their own), else the first.
  local all e win=0 found_win="" esp1="" esp1_others="" esp1_free=0 o free
  all=$(lsblk -l -n -p -o NAME,PARTTYPE "$disk" 2>/dev/null \
    | awk -v t="$FRIDAY_ESP_TYPE" 'tolower($2) == t { print $1 }')
  if lsblk -l -n -o FSTYPE "$disk" 2>/dev/null | grep -qx BitLocker; then echo bitlocker=1; else echo bitlocker=0; fi
  echo "esps=$(grep -c . <<<"$all" || true)"
  mnt=$(mktemp -d /tmp/friday-esp.XXXXXX)
  for e in $all; do
    mount -o ro "$e" "$mnt" 2>/dev/null || continue
    o=""
    for d in "$mnt"/EFI/*/; do
      [[ -d "$d" ]] || continue
      d=$(basename "$d")
      [[ "${d,,}" =~ ^(microsoft|boot|systemd|linux)$ ]] || o+="${o:+, }$d"
    done
    free=$(df -m --output=avail "$mnt" | tail -n1 | xargs)
    if [[ -z "$found_win" && -e "$mnt/EFI/Microsoft/Boot/bootmgfw.efi" ]]; then
      found_win="$e|$o|$free"
    fi
    [[ -n "$esp1" ]] || { esp1="$e"; esp1_others="$o"; esp1_free="$free"; }
    umount "$mnt" || true
  done
  rmdir "$mnt" 2>/dev/null || true
  if [[ -n "$found_win" ]]; then
    IFS='|' read -r esp others free <<<"$found_win"
    win=1
  else
    esp="$esp1" others="$esp1_others" free="$esp1_free"
  fi
  echo "esp=$esp"
  echo "windows=$win"
  echo "others=$others"
  echo "esp_free_mb=${free:-0}"
}

# The size FRIDAY suggests, in MB: 80 GB, or 60 GB plus room for the
# biggest local model this computer runs twice over; never more than the
# free space (rounded down to a GB), 0 when even the minimum doesn't fit.
#   friday_size_recommend <free_mb> [model_mb]
friday_size_recommend() {
  local free="$1" model="${2:-0}" rec
  (( free >= FRIDAY_MIN_MB )) || { echo 0; return; }
  rec=$(( 61440 + 2 * model ))
  (( rec < 81920 )) && rec=81920
  rec=$(( (rec + 10239) / 10240 * 10240 ))
  (( rec > free )) && rec=$(( free / 1024 * 1024 ))
  echo "$rec"
}

# Megabytes as whole gigabytes ("82 GB").
friday_gb() { awk -v m="$1" 'BEGIN { printf "%.0f GB", m / 1024 }'; }

# One line on how a size will work out; fails when it can't be used.
#   friday_size_advice <size_mb> <free_mb> [model_mb]
friday_size_advice() {
  local size="$1" free="$2" model="${3:-0}" rec ai
  if (( size > free )); then echo "There's only $(friday_gb "$free") free."; return 1; fi
  if (( size < FRIDAY_MIN_MB )); then echo "Too small: FRIDAY needs at least $(friday_gb "$FRIDAY_MIN_MB")."; return 1; fi
  rec=$(friday_size_recommend "$free" "$model")
  ai=$(( FRIDAY_MIN_MB + 2 * model ))
  if (( size < 61440 )); then
    echo -n "Tight: enough to try FRIDAY, with little room for your files"
    if (( model > 0 && size < ai )); then echo " and too little for local AI."; else echo "."; fi
  elif (( model > 0 && size < ai )); then
    echo "Fine for FRIDAY, but tight for local AI: its model is $(friday_gb "$model"), and updates need room for a second copy."
  elif (( size > 2 * rec && size > 204800 )); then
    echo "Generous: about $(friday_gb $(( size - rec ))) will likely sit unused, and the other system can't use it."
  else
    echo "Good: room for FRIDAY and your files$( (( model > 0 )) && echo ", and local AI")."
  fi
}

# Puts FRIDAY into the largest free block of a GPT disk: a 1 GiB XBOOTLDR
# partition (kernels; the EFI partition is often too small for them) and a
# root partition of the rest of <size_mb>. Backs up the partition table
# first and checks afterwards that every partition that was there is
# exactly as it was. Sets FRIDAY_BOOT_PART and FRIDAY_ROOT_PART.
#   friday_disk_alongside <disk> <size_mb> <backup-file>
friday_disk_alongside() {
  local disk="$1" size="$2" backup="$3" before after used n boot_n="" root_n="" line
  sgdisk --backup="$backup" "$disk" >/dev/null || { echo "Couldn't back up the partition table of $disk."; return 1; }
  before=$(sfdisk -d "$disk" | grep '^/dev/')
  used=$(sgdisk -p "$disk" | awk '$1 ~ /^[0-9]+$/ { print $1 }')
  for n in $(seq 1 128); do
    grep -qx "$n" <<<"$used" && continue
    if [[ -z "$boot_n" ]]; then boot_n=$n; else root_n=$n; break; fi
  done
  [[ -n "$root_n" ]] || { echo "$disk has no free partition slots."; return 1; }
  # Start and end 0: the largest free block, so nothing in use is touched.
  sgdisk -n "$boot_n:0:+1024M" -t "$boot_n:$FRIDAY_XBOOTLDR_TYPE" -c "$boot_n:FRIDAY boot" \
         -n "$root_n:0:+$(( size - 1024 ))M" -t "$root_n:$FRIDAY_ROOT_TYPE" -c "$root_n:FRIDAY OS" \
         "$disk" >/dev/null || { echo "Couldn't add FRIDAY's partitions to $disk."; return 1; }
  partprobe "$disk" 2>/dev/null || partx -u "$disk" 2>/dev/null || true
  udevadm settle 2>/dev/null || true
  after=$(sfdisk -d "$disk" | grep '^/dev/')
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    grep -qxF -- "$line" <<<"$after" || { echo "A partition that was there changed: $line"; return 1; }
  done <<<"$before"
  FRIDAY_BOOT_PART=$(friday_part_dev "$disk" "$boot_n")
  FRIDAY_ROOT_PART=$(friday_part_dev "$disk" "$root_n")
  [[ "$(lsblk -n -d -o PARTTYPE "$FRIDAY_BOOT_PART" 2>/dev/null)" == "$FRIDAY_XBOOTLDR_TYPE" ]] \
    || { echo "The new boot partition $FRIDAY_BOOT_PART isn't there."; return 1; }
  [[ -b "$FRIDAY_ROOT_PART" ]] || { echo "The new partition $FRIDAY_ROOT_PART isn't there."; return 1; }
}

# The files on an EFI partition with their checksums (the check that
# installing only added to it). systemd-boot's own folder may be updated.
#   friday_esp_files <mounted-esp>
friday_esp_files() {
  (cd "$1" && find . -type f ! -ipath './EFI/systemd/*' -print0 | sort -z | xargs -0 -r sha256sum)
}
