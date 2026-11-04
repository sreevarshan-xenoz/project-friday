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
" ██████╗ ██████╗ ██╗██████╗  █████╗ ██╗   ██╗" \
" ██╔════╝██╔══██╗██║██╔══██╗██╔══██╗╚██╗ ██╔╝" \
" █████╗  ██████╔╝██║██║  ██║███████║ ╚████╔╝ " \
" ██╔══╝  ██╔══██╗██║██║  ██║██╔══██║  ╚██╔╝  " \
" ██║     ██║  ██║██║██████╔╝██║  ██║   ██║   " \
" ╚═╝     ╚═╝  ╚═╝╚═╝╚═════╝ ╚═╝  ╚═╝   ╚═╝   "
  echo
  gum style --width "$cols" --foreground "$FRIDAY_SECONDARY" --bold --align center "$subtitle"
  gum style --width "$cols" --foreground "$FRIDAY_MUTED" --align center "────────────────────────────────────────────────────────────"
  echo
}

# FRIDAY "speaks": a soft purple line typed out character by character.
friday_say() {
  local text="$*"
  if ! friday_is_tty; then
    printf '  FRIDAY › %s\n' "$text"
    return
  fi
  printf '  \e[38;2;187;154;247mFRIDAY ›\e[0m \e[38;2;192;202;245m'
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

friday_ok()   { gum style --foreground "$FRIDAY_SUCCESS" "  ✓ $*"; }
friday_warn() { gum style --foreground "$FRIDAY_WARNING" "  ! $*"; }
friday_err()  { gum style --foreground "$FRIDAY_ERROR" "  ✗ $*"; }
friday_head() { gum style --foreground "$FRIDAY_ACCENT" --bold "$*"; }
friday_note() { gum style --foreground "$FRIDAY_MUTED" "$*"; }

# Progress rail: friday_rail <current-index> <label>...
friday_rail() {
  local current="$1"; shift
  local out="" i=1 label
  for label in "$@"; do
    if (( i < current )); then out+="✓ $label   "
    elif (( i == current )); then out+="● $label   "
    else out+="○ $label   "; fi
    i=$((i + 1))
  done
  gum style --width "$(tput cols 2>/dev/null || echo 80)" --foreground "$FRIDAY_MUTED" --align center "${out%   }"
  echo
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
