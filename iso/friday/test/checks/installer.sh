#!/usr/bin/env bash
# Live installer: input validators, a scripted configurator run, and the
# installer's progress/failure screens against a fake pacman log.
source "$(dirname "$0")/lib.sh"
CHECKS="$(cd "$(dirname "$0")" && pwd)"
ROOT="$AIROOTFS/root"

echo "validators"
# shellcheck source=/dev/null
source "$ROOT/friday-tui.sh"
expect "hostname 'friday'"           friday_valid_hostname friday
expect "hostname 'my-box2'"          friday_valid_hostname my-box2
refuse "hostname 'my box'"           friday_valid_hostname "my box"
refuse "hostname 'MyBox'"            friday_valid_hostname MyBox
refuse "hostname '-bad'"             friday_valid_hostname -bad
refuse "hostname of 64 chars"        friday_valid_hostname "$(printf 'a%.0s' {1..64})"
expect "username 'friday'"           friday_valid_username friday
refuse "username 'root'"             friday_valid_username root
refuse "username 'Bob'"              friday_valid_username Bob
refuse "username '9lives'"           friday_valid_username 9lives
expect "timezone Asia/Kolkata"       friday_valid_timezone Asia/Kolkata
refuse "timezone Asia/Kolkatta"      friday_valid_timezone Asia/Kolkatta
refuse "timezone path traversal"     friday_valid_timezone ../../etc/passwd
expect "api key shape"               friday_valid_api_key AIzaSyA-abc_123.xyz
refuse "api key with space"          friday_valid_api_key "has space key"
expect "timezone list never empty"   test -n "$(friday_timezones | head -n1)"
expect "keymap list never empty"     test -n "$(friday_keymaps | head -n1)"

echo "keyboard: console keymap to desktop layout"
K="$WORK/kbd"; mkdir -p "$K"
printf '# consolelayout\txlayout\txmodel\txvariant\txoptions\n' >"$K/map"
printf 'uk\t\tgb\tpc105\t-\tterminate:ctrl_alt_bksp\n' >>"$K/map"
printf 'dvorak\t\tus\tpc105\tdvorak\tterminate:ctrl_alt_bksp\n' >>"$K/map"
printf 'de\t\tde\tpc105\t-\t-\n' >>"$K/map"
printf 'ru\t\tru,us\tpc105\t-\tgrp:shifts_toggle\n' >>"$K/map"
printf '! model\n  pc105  Generic\n\n! layout\n  us  English\n  pl  Polish\n\n! variant\n  zz  us: x\n' >"$K/base.lst"
xkb() { friday_xkb_for_keymap "$1" "$K/map" "$K/base.lst"; }
[[ $(xkb uk) == "gb pc105 - terminate:ctrl_alt_bksp" ]] && pass "uk types as gb" || fail "uk: $(xkb uk)"
[[ $(xkb dvorak) == "us pc105 dvorak terminate:ctrl_alt_bksp" ]] && pass "dvorak keeps its variant" || fail "dvorak: $(xkb dvorak)"
[[ $(xkb de-latin1-nodeadkeys) == "de pc105 - -" ]] && pass "a suffixed keymap falls back to its base" || fail "de-latin1-nodeadkeys: $(xkb de-latin1-nodeadkeys)"
[[ $(xkb ru) == "ru,us pc105 - grp:shifts_toggle" ]] && pass "ru keeps a Latin layout to switch to" || fail "ru: $(xkb ru)"
[[ $(xkb pl2) == "us pc105 - -" && $(xkb pl) == "pl pc105 - -" ]] && pass "an XKB layout name stands for itself" || fail "pl: $(xkb pl)"
[[ $(xkb zz) == "us pc105 - -" ]] && pass "an unknown keymap types as us" || fail "zz: $(xkb zz)"
[[ $(friday_xkb_for_keymap uk /nonexistent /nonexistent) == "us pc105 - -" ]] && pass "no map files: us" || fail "no map files"
if [[ -r /usr/share/systemd/kbd-model-map ]]; then
  sys() { friday_xkb_for_keymap "$1" | cut -d' ' -f1,3; }
  [[ $(sys uk) == "gb -" && $(sys de-latin1) == "de -" && $(sys dvorak) == "us dvorak" ]] \
    && pass "systemd's map: uk gb, de-latin1 de, dvorak us/dvorak" \
    || fail "systemd's map: uk $(sys uk), de-latin1 $(sys de-latin1), dvorak $(sys dvorak)"
else skip "systemd's kbd-model-map (not installed)"; fi

echo "keyboard: the installer applies it"
# The installer's keyboard block, run against a scratch root.
mkdir -p "$K/root/etc"
awk '/^# Keyboard:/ {on = 1} on {print} on && /^EOF_XKB$/ {exit}' "$ROOT/installer" \
  | sed "s#/mnt/#$K/root/#g" >"$K/apply.sh"
if [[ -s "$K/apply.sh" ]] && KEYBOARD=uk bash -c "source '$ROOT/friday-tui.sh'; source '$K/apply.sh'"; then
  vc=$(cat "$K/root/etc/vconsole.conf" 2>/dev/null)
  contains "vconsole.conf keeps the console keymap" "$vc" "KEYMAP=uk"
  contains "vconsole.conf has the desktop layout" "$vc" "XKBLAYOUT=gb"
  contains "00-keyboard.conf has it for localed (niri, login screen)" \
    "$(cat "$K/root/etc/X11/xorg.conf.d/00-keyboard.conf" 2>/dev/null)" 'Option "XkbLayout" "gb"'
  if need lua; then
    # Omarchy's input.lua (FRIDAY OS's Hyprland input) reading that vconsole.conf.
    got=$(lua -e "
      local real = io.open
      io.open = function(p, m) if p == '/etc/vconsole.conf' then p = '$K/root/etc/vconsole.conf' end return real(p, m) end
      hl = { config = function(t) print(t.input.kb_layout .. '/' .. t.input.kb_variant) end }
      dofile('$AIROOTFS/usr/share/omarchy/default/hypr/input.lua')" 2>&1 | head -1)
    [[ "$got" == "gb/" ]] && pass "Hyprland (Omarchy's input.lua) types gb" || fail "Hyprland's layout: $got"
  else skip "Hyprland's layout (needs lua)"; fi
else fail "the installer's keyboard block runs"; fi
if need lua; then
  # The Dots-Hyprland option's custom file, as the installer appends it.
  awk '/^-- FRIDAY OS: the system keyboard layout/ {on = 1} on && /^EOF_DOTS_KB$/ {exit} on {print}' "$ROOT/installer" >"$K/dots.lua"
  got=$(lua -e "
    local real = io.open
    io.open = function(p, m) if p == '/etc/vconsole.conf' then p = '$K/root/etc/vconsole.conf' end return real(p, m) end
    hl = { config = function(t) print(t.input.kb_layout .. '/' .. t.input.kb_variant) end }
    dofile('$K/dots.lua')" 2>&1 | head -1)
  [[ -s "$K/dots.lua" && "$got" == "gb/" ]] && pass "the Dots-Hyprland option types gb too" || fail "Dots-Hyprland's layout: $got"
else skip "Dots-Hyprland's layout (needs lua)"; fi
kb_line=$(grep -n '^# Keyboard:' "$ROOT/installer" | cut -d: -f1)
initrd_line=$(grep -n 'mkinitcpio -P' "$ROOT/installer" | head -1 | cut -d: -f1)
[[ -n "$kb_line" && -n "$initrd_line" ]] && (( kb_line < initrd_line )) \
  && pass "the keymap is set before the initramfs (disk-unlock prompt)" \
  || fail "vconsole.conf must be written before mkinitcpio -P"

echo "disk encryption: mkinitcpio hook"
C="$WORK/crypt"; mkdir -p "$C"
printf 'MODULES=()\nHOOKS=(base systemd autodetect microcode modconf kms keyboard sd-vconsole block filesystems fsck)\n' >"$C/sd.conf"
printf 'HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block filesystems fsck)\n' >"$C/bb.conf"
printf 'HOOKS=(base udev block fsck)\n' >"$C/bad.conf"
params=$(friday_add_crypt_hook "$C/sd.conf" 1234-ab cryptroot)
contains "systemd initramfs gets sd-encrypt before filesystems" "$(cat "$C/sd.conf")" "block sd-encrypt filesystems"
[[ "$params" == "rd.luks.name=1234-ab=cryptroot root=/dev/mapper/cryptroot" ]] && pass "systemd boot params" || fail "systemd boot params: $params"
params=$(friday_add_crypt_hook "$C/bb.conf" 1234-ab cryptroot)
contains "busybox initramfs gets encrypt before filesystems" "$(cat "$C/bb.conf")" "block encrypt filesystems"
[[ "$params" == "cryptdevice=UUID=1234-ab:cryptroot root=/dev/mapper/cryptroot" ]] && pass "busybox boot params" || fail "busybox boot params: $params"
friday_add_crypt_hook "$C/sd.conf" 1234-ab cryptroot >/dev/null
[[ $(grep -o sd-encrypt "$C/sd.conf" | wc -l) == 1 ]] && pass "hook insertion is idempotent" || fail "sd-encrypt added twice"
refuse "fails when there is no filesystems hook" friday_add_crypt_hook "$C/bad.conf" 1234-ab cryptroot

echo "disk encryption: real LUKS2 round trip (installer's commands)"
if need cryptsetup && [[ $EUID -eq 0 ]]; then
  img="$C/disk.img"; truncate -s 64M "$img"
  pw='p@ss word"with$odd'
  if printf '%s' "$pw" | cryptsetup luksFormat --type luks2 --batch-mode --pbkdf-memory 32768 --key-file=- "$img" 2>"$C/err"; then
    pass "luksFormat with the passphrase on stdin"
    expect "luksUUID readable" test -n "$(cryptsetup luksUUID "$img")"
    expect "the same passphrase unlocks it" bash -c "printf '%s' '$pw' | cryptsetup open --test-passphrase --key-file=- '$img'"
    refuse "a wrong passphrase does not" bash -c "printf 'nope' | cryptsetup open --test-passphrase --key-file=- '$img'"
  else skip "LUKS round trip ($(tail -n1 "$C/err"))"; fi
else skip "LUKS round trip (needs root + cryptsetup)"; fi

echo "unattended runs (no terminal)"
# The FEATURES step's settings: a default install writes nothing; each choice
# lands in the file the CLI or daemon already reads.
feat() { # feat <dir> VAR=value...  → runs friday_write_features with those choices
  local d="$1"; shift
  env "$@" bash -c "source '$ROOT/friday-tui.sh'; friday_write_features '$d'"
}
FD="$WORK/features"; rm -rf "$FD"
feat "$FD/default"
if [[ -z "$(ls -A "$FD/default")" ]]; then pass "default features write nothing (the phone is offered)"; else fail "default features wrote: $(ls "$FD/default")"; fi
feat "$FD/nophone" FEAT_PHONE=false
if [[ "$(ls -A "$FD/nophone")" == "no-phone-offer" ]]; then pass "declining phone pairing writes the no-phone-offer flag"; else fail "phone pairing declined but wrote: $(ls "$FD/nophone")"; fi
feat "$FD/off" FEAT_DESKTOP=false FEAT_TASKS=false FEAT_APPNAME=false FEAT_SCREENTEXT=false
contains "desktop control off is written"   "$(cat "$FD/off/friday.env")" "FRIDAY_DESKTOP=0"
contains "scheduled tasks off is written"   "$(cat "$FD/off/friday.env")" "FRIDAY_STANDING_TASKS=0"
contains "window text off is written"       "$(cat "$FD/off/friday.env")" "FRIDAY_SCREEN_TEXT=0"
expect  "app name off leaves the flag file" test -e "$FD/off/no-app-context"
[[ "$(stat -c %a "$FD/off/friday.env")" == 600 ]] && pass "friday.env is owner-only" || fail "friday.env permissions are $(stat -c %a "$FD/off/friday.env")"
mkdir -p "$FD/keep"; printf 'GEMINI_API_KEY=k\n' >"$FD/keep/friday.env"
feat "$FD/keep" FEAT_SCREENTEXT=false
contains "an existing provider key is kept" "$(cat "$FD/keep/friday.env")" "GEMINI_API_KEY=k"
contains "and the choice is added"          "$(cat "$FD/keep/friday.env")" "FRIDAY_SCREEN_TEXT=0"
# The rail keeps to one line on a narrow screen, naming only the current step.
narrow=$(COLUMNS=60 TERM=linux bash -c "source '$ROOT/friday-tui.sh'; tput() { echo 60; }; friday_rail 7 NETWORK DISK REGION IDENTITY SECURITY LOOK FEATURES AI REVIEW" | sed 's/\x1b\[[0-9;]*m//g' | grep -v '^$')
if [[ $(wc -l <<<"$narrow") == 1 && "$narrow" == *FEATURES* && "$narrow" != *NETWORK* ]]; then pass "the rail shortens on a narrow screen"; else fail "the rail on 60 columns: $narrow"; fi
wide=$(TERM=linux bash -c "source '$ROOT/friday-tui.sh'; tput() { echo 120; }; friday_rail 7 NETWORK DISK REGION IDENTITY SECURITY LOOK FEATURES AI REVIEW" | sed 's/\x1b\[[0-9;]*m//g' | grep -v '^$')
[[ "$wide" == *NETWORK* && "$wide" == *REVIEW* ]] && pass "the rail names every step when it fits" || fail "the rail on 120 columns: $wide"

expect "banner and rail work without TERM" env -u TERM bash -c \
  "source '$ROOT/friday-tui.sh'; set -e; friday_banner TEST; friday_rail 2 A B C; friday_say hi" </dev/null

# Spoken lines: silent unless the person asked for the voice.
SV=$(mktemp -d); mkdir -p "$SV/bin" "$SV/voice"; printf '#!/bin/sh\necho "$*" >>"%s/said"\n' "$SV" >"$SV/bin/friday-ctl"; chmod +x "$SV/bin/friday-ctl"
speak() { PATH="$SV/bin:$PATH" bash -c "source '$ROOT/friday-tui.sh'; FRIDAY_LIVE_VOICE='$SV/voice'; friday_speak hello; wait" </dev/null; sleep 0.3; }
speak; [[ ! -e "$SV/said" ]] && pass "no voice requested: nothing is spoken" || fail "spoke without being asked"
: >"$SV/voice/on"; speak; grep -qx "say hello" "$SV/said" 2>/dev/null && pass "voice on: the line goes to friday-ctl say" || fail "voice on but nothing was said"
rm -rf "$SV"

echo "no Omarchy name where a person reads it"
ISO_DIR="$ROOT/../.."
if grep -qi omarchy "$ISO_DIR/efiboot/loader/entries/01-archiso-linux.conf" "$ISO_DIR/airootfs/etc/motd" \
  || grep -i 'menuentry' "$ISO_DIR/grub/grub.cfg" | grep -qi omarchy \
  || grep -i 'phase_msg\|^title' "$ROOT/installer" | grep -qi omarchy; then
  fail "the boot menu, login text or installer step names still say Omarchy"
else pass "boot menu, login text and installer steps do not say Omarchy"; fi

echo "boot splash"
[[ -s "$ROOT/../usr/share/friday-installer/plymouth-logo.png" ]] && pass "FRIDAY's splash logo ships in the ISO" || fail "plymouth-logo.png is missing"
grep -q 'friday-installer/plymouth-logo.png /mnt/usr/share/plymouth/themes/omarchy/logo.png' "$ROOT/installer" && pass "the installer puts it into the installed splash theme" || fail "the installer does not use the splash logo"

echo "login security"
guarded=$(awk '/if \[\[ "\$AUTOLOGIN" == "true" \]\]/ {g=1} /EOF_SDDM_AUTO > \/mnt\/etc\/sddm.conf.d\/autologin.conf/ {print (g ? "guarded" : "unguarded"); exit}' "$ROOT/installer")
if [[ "$guarded" == guarded ]]; then pass "SDDM autologin is only written when AUTOLOGIN=true"
else fail "installer writes SDDM autologin unconditionally"; fi
expect "ESP mounted root-only (fmask/dmask 0077)" grep -q 'mount -o fmask=0077,dmask=0077 "$BOOT_PART" /mnt/boot' "$ROOT/installer"
expect "AUTOLOGIN defaults to false" grep -q 'AUTOLOGIN="${AUTOLOGIN:-false}"' "$ROOT/installer"
expect "root is locked unless it's the VM debug install" grep -q 'arch-chroot /mnt passwd -l root' "$ROOT/installer"
if grep -q '^echo "root:\$PASSWORD" | arch-chroot /mnt chpasswd' "$ROOT/installer"; then
  fail "root gets the person's password unconditionally"
else pass "root doesn't get the person's password"; fi

echo "disk safety: the installer refuses before erasing anything"
# The live disk: the USB stick's whole disk, not its partition (nor the two
# names run together), and the device itself for a CD.
live_disk() { # live_disk <findmnt source> <lsblk parent>
  local B="$WORK/live-disk-stub"; mkdir -p "$B"
  printf '#!/bin/bash\necho %s\n' "$1" >"$B/findmnt"
  printf '#!/bin/bash\necho %s\n' "$2" >"$B/lsblk"
  chmod +x "$B/findmnt" "$B/lsblk"
  PATH="$B:$PATH" bash -c 'source "$1" >/dev/null 2>&1; friday_live_disk' _ "$ROOT/friday-tui.sh"
}
[[ "$(live_disk /dev/sdb1 sdb)" == /dev/sdb ]] && pass "booted from a USB stick: its disk is /dev/sdb" \
  || fail "live disk for /dev/sdb1 is '$(live_disk /dev/sdb1 sdb)', not /dev/sdb"
[[ "$(live_disk /dev/nvme1n1p1 nvme1n1)" == /dev/nvme1n1 ]] && pass "an NVMe stick: /dev/nvme1n1" \
  || fail "live disk for /dev/nvme1n1p1 is '$(live_disk /dev/nvme1n1p1 nvme1n1)'"
[[ "$(live_disk /dev/sr0 "")" == /dev/sr0 ]] && pass "booted from a CD: /dev/sr0" \
  || fail "live disk for a CD is '$(live_disk /dev/sr0 "")'"
if need gum; then
  F="$WORK/installer-not-a-disk"; : >"$F"
  refused() { # refused <expected words> <env...>: the installer stops, says why and erases nothing
    local want=$1 out; shift
    out=$(env -i PATH="$PATH" HOME="$HOME" TERM=dumb FRIDAY_FAST=1 FRIDAY_ASSUME_UEFI=1 PASSWORD=x "$@" \
      bash "$ROOT/installer" 2>&1 </dev/null) && return 1
    grep -q "Nothing was written" <<<"$out" && grep -q "$want" <<<"$out"
  }
  refused "No disk was chosen" && pass "no TARGET_DISK: refused" || fail "the installer runs with no TARGET_DISK"
  refused "wasn't confirmed" TARGET_DISK="$F" && pass "TARGET_DISK without ERASE_DISK: refused" \
    || fail "the installer erases a disk nobody confirmed"
  refused "isn't a whole disk" TARGET_DISK="$F" ERASE_DISK="$F" && pass "a target that isn't a whole disk: refused" \
    || fail "the installer accepts a target that isn't a whole disk"
  refused "wasn't confirmed" INSTALL_MODE=alongside TARGET_DISK="$F" ERASE_DISK="$F" \
    && pass "alongside without ALONGSIDE_DISK: refused (ERASE_DISK doesn't stand in)" \
    || fail "the installer goes alongside a disk nobody confirmed"
  refused "FRIDAY_SIZE_MB" INSTALL_MODE=alongside TARGET_DISK="$F" ALONGSIDE_DISK="$F" FRIDAY_SIZE_MB=1000 \
    && pass "alongside with less than 40 GB: refused" || fail "the installer goes alongside in 1 GB"
fi

echo "dual boot: the installer keeps what the partition step found"
# In $( ) its FRIDAY_BOOT_PART/FRIDAY_ROOT_PART were lost (a VM install stopped).
if grep -qE '\$\(friday_disk_alongside' "$ROOT/installer"; then fail "friday_disk_alongside runs in a subshell"
else pass "friday_disk_alongside runs in the installer's own shell"; fi

echo "dual boot: how much space FRIDAY suggests and how it judges a size"
# (friday-tui.sh is sourced above, for the validators)
[[ $(friday_size_recommend 102400) == 81920 ]] && pass "100 GB free: 80 GB suggested" || fail "100 GB free: $(friday_size_recommend 102400)"
[[ $(friday_size_recommend 70000 8192) == 69632 ]] && pass "never more than what's free" || fail "70000 free: $(friday_size_recommend 70000 8192)"
[[ $(friday_size_recommend 30000) == 0 ]] && pass "under 40 GB free: nothing fits" || fail "30000 free: $(friday_size_recommend 30000)"
[[ $(friday_size_recommend 409600 20480) == 102400 ]] && pass "a 20 GB local model: 100 GB suggested" || fail "with a 20 GB model: $(friday_size_recommend 409600 20480)"
friday_size_advice 30000 102400 >/dev/null && fail "30 GB accepted" || pass "30 GB: too small"
friday_size_advice 110000 102400 >/dev/null && fail "more than the free space accepted" || pass "more than what's free: refused"
contains "50 GB with local AI: tight" "$(friday_size_advice 51200 102400 8192)" "too little for local AI"
contains "80 GB: good" "$(friday_size_advice 81920 102400)" "Good: room for FRIDAY"
contains "300 GB of 400: generous, says what sits unused" "$(friday_size_advice 307200 409600)" "Generous: about 220 GB"
[[ $(friday_part_dev /dev/nvme0n1 5) == /dev/nvme0n1p5 && $(friday_part_dev /dev/sda 5) == /dev/sda5 ]] \
  && pass "partition names: nvme0n1p5, sda5" || fail "partition names"

echo "configurator flow (pty)"
if need gum python3; then
  W="$WORK/installer"; mkdir -p "$W/stub"
  cp "$ROOT/configurator" "$ROOT/friday-tui.sh" "$W/"
  sed -i "s#/root/installer#$W/stub/installer#" "$W/configurator"
  # STUB_WINDOWS=1: the disk has Windows (BitLocker on) and 100 GB free.
  cat >"$W/stub/lsblk" <<'EOF'
#!/bin/bash
if [[ "$*" == *"-d -n -o NAME,TYPE,SIZE,MODEL"* ]]; then echo "vda disk 25G QEMU_HARDDISK"; exit; fi
if [[ -n "${STUB_WINDOWS:-}" ]]; then
  case "$*" in
    *"-d -o PTTYPE"*) echo gpt ;;
    *"-o TYPE"*) printf 'disk\npart\npart\n' ;;
    *"NAME,PARTTYPE"*) echo "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b" ;;
    *"-o FSTYPE"*) printf 'vfat\nBitLocker\n' ;;
  esac
  exit
fi
echo ext4
EOF
  cat >"$W/stub/sgdisk" <<'EOF'
#!/bin/bash
case "$1" in -F) echo 2048 ;; -E) echo 209717247 ;; esac
EOF
  printf '#!/bin/bash\necho 512\n' >"$W/stub/blockdev"
  printf '#!/bin/bash\nmkdir -p "${@: -1}/EFI/Microsoft/Boot"; : >"${@: -1}/EFI/Microsoft/Boot/bootmgfw.efi"\n' >"$W/stub/mount"
  printf '#!/bin/bash\nrm -rf "$1/EFI"\n' >"$W/stub/umount"
  printf '#!/bin/bash\nprintf "Avail\\n  80\\n"\n' >"$W/stub/df"
  printf '#!/bin/bash\nexit 1\n' >"$W/stub/findmnt"
  printf '#!/bin/bash\nexit 0\n' >"$W/stub/curl"   # pretend we're online
  cat >"$W/stub/installer" <<'EOF'
#!/bin/bash
echo "INSTALLER-ENV disk=$TARGET_DISK erase=$ERASE_DISK mode=$INSTALL_MODE alongside=$ALONGSIDE_DISK size=$FRIDAY_SIZE_MB bootdefault=$BOOT_DEFAULT bitlocker_ack=$BITLOCKER_ACK host=$FRIDAY_HOSTNAME user=$USERNAME pw=$PASSWORD tz=$TIMEZONE kb=$KEYBOARD ai=[$FRIDAY_AI_PROVIDERS] keys=[$(cut -d= -f1 <<<"$FRIDAY_AI_KEYS" | xargs)] local=[$FRIDAY_LOCAL_ENGINE:$FRIDAY_LOCAL_MODEL:$FRIDAY_LOCAL_BUDGET_MB] autologin=$AUTOLOGIN encrypt=$ENCRYPT interactive=$FRIDAY_INTERACTIVE session=$SESSION style=$SHELL_STYLE pet=$SHELL_PET desktop=$FEAT_DESKTOP tasks=$FEAT_TASKS appname=$FEAT_APPNAME screentext=$FEAT_SCREENTEXT browsers=[$BROWSERS] default_browser=$DEFAULT_BROWSER editors=[$EDITORS] agents=[$AGENTS] files=$FILE_MANAGER term=$TERMINAL_APP video=$VIDEO_PLAYER pdf=$PDF_VIEWER more=[$MORE_APPS] git=[$GIT_NAME|$GIT_EMAIL|$GITHUB_USER] ghbackup=$GITHUB_BACKUP"
EOF
  # friday-git is on the live ISO: the real CLI, on a fake 16 GB machine with no GPU.
  mkdir -p "$W/hw/proc"
  printf 'MemTotal:       16777216 kB\n' >"$W/hw/proc/meminfo"
  printf 'processor\t: 0\nflags\t\t: avx2\n' >"$W/hw/proc/cpuinfo"
  printf '#!/bin/bash\nFRIDAY_HW_ROOT=%q exec bash %q "$@"\n' "$W/hw" "$REPO_ROOT/packaging/linux/friday" >"$W/stub/friday"
  chmod +x "$W"/stub/* "$W/configurator"
  if python3 "$CHECKS/installer_flow.py" "$W/configurator" "$W/stub"; then :; else FAILS=$((FAILS + 1)); fi
  echo "configurator flow: several providers and local AI (pty)"
  if python3 "$CHECKS/installer_flow.py" "$W/configurator" "$W/stub" multi; then :; else FAILS=$((FAILS + 1)); fi
  echo "configurator flow: alongside Windows (pty)"
  if STUB_WINDOWS=1 python3 "$CHECKS/installer_flow.py" "$W/configurator" "$W/stub" alongside; then :; else FAILS=$((FAILS + 1)); fi
else skip "configurator flow (needs gum + python3)"; fi

echo "installer: AI keys and local AI"
W="$WORK/ai-section"; rm -rf "$W"; mkdir -p "$W/mnt/home/alice"
{
  echo 'set -euo pipefail; USER_HOME=/home/alice'
  echo "FRIDAY_AI_KEYS=\$'gemini=gem-key-123456\\ngroq=groq-key-123456\\n'"
  echo 'FRIDAY_LOCAL_ENGINE=ollama FRIDAY_LOCAL_BUDGET_MB=8192 FRIDAY_LOCAL_MODEL=gemma3:4b'
  awk '/^# FRIDAY AI provider credentials, loaded by/,/downloaded after the first login/' "$ROOT/installer"
  echo fi
} | sed "s#/mnt#$W/mnt#g" >"$W/section.sh"
out=$(bash "$W/section.sh" 2>&1)
envf="$W/mnt/home/alice/.config/friday/friday.env"
contains "both keys saved" "$(cat "$envf" 2>/dev/null)" $'GEMINI_API_KEY=gem-key-123456\nGROQ_API_KEY=groq-key-123456'
contains "screen context goes to the first vision provider" "$(cat "$envf" 2>/dev/null)" "FRIDAY_CONTEXT_PROVIDER=gemini"
[[ "$(stat -c %a "$envf" 2>/dev/null)" == 600 ]] && pass "friday.env is 0600" || fail "friday.env mode $(stat -c %a "$envf" 2>/dev/null)"
[[ "$out" != *key-123456* ]] && pass "keys never reach the log" || fail "a key was printed"
contains "local AI choice saved, pending" "$(cat "$W/mnt/home/alice/.config/friday/local-ai.conf" 2>/dev/null)" $'engine=ollama\nbudget_mb=8192\nmodel=gemma3:4b\nstate=pending'
[[ -L "$W/mnt/home/alice/.config/systemd/user/default.target.wants/friday-local-ai.service" ]] \
  && pass "the first-login download is enabled for the user" || fail "friday-local-ai.service not enabled"

echo "installer: browser extension flags"
contains "chromium loads the extension" "$(cat "$ROOT/../etc/skel/.config/chromium-flags.conf")" "/usr/share/friday/browser-extension"
contains "brave loads the extension" "$(cat "$ROOT/../etc/skel/.config/brave-flags.conf")" "--load-extension=/usr/share/friday/browser-extension"
contains "brave ozone hint auto" "$(cat "$ROOT/../etc/skel/.config/brave-flags.conf")" "--ozone-platform-hint=auto"
contains "vivaldi loads the extension" "$(cat "$ROOT/../etc/skel/.config/vivaldi-stable.conf")" "--load-extension=/usr/share/friday/browser-extension"
contains "vivaldi ozone hint auto" "$(cat "$ROOT/../etc/skel/.config/vivaldi-stable.conf")" "--ozone-platform-hint=auto"

echo "installer: browser packages and default browser"
W="$WORK/browser-section"; rm -rf "$W"; mkdir -p "$W/mnt/home/alice"
cat >"$W/test-browser.sh" <<'EOF'
set -euo pipefail
USER_HOME=/home/alice
USERNAME=alice
LOG="$WORK/browser-section/pacstrap.log"
pacstrap_with_retry() { shift; echo "pacstrap: $*" >> "$LOG"; }
# arch-chroot: log what runs as whom; a build of a package named in
# FAIL_AUR fails.
# Like the real one, each call gets a fresh, empty /tmp: a build folder
# there would be gone by the next call.
arch-chroot() {
  shift
  rm -rf "$WORK/browser-section/mnt/tmp"; mkdir -p "$WORK/browser-section/mnt/tmp"
  case "$*" in
    *makepkg*) echo "build: $*" >> "$LOG"; [[ "$*" != *" /tmp/"* ]] && [[ -z "${FAIL_AUR:-}" || "$*" != *"$FAIL_AUR"* ]] ;;
    *) echo "chroot: $*" >> "$LOG" ;;
  esac
}
friday_note() { echo "$*"; }
friday_apps_line() { echo "apps: $*" >> "$LOG"; }
friday_app_name() { echo "$1"; }
live_status_start() { :; }
live_status_stop() { :; }
EOF
browsers() {
  {
    cat "$W/test-browser.sh"
    echo "BROWSERS=\"$1\" DEFAULT_BROWSER=$2"
    awk '/^# Web browsers chosen in addition to Chromium/,/^# \(end of the apps from the Arch repos\)/' "$ROOT/installer"
    sed -n '/^friday_aur_install() {/,/^if (( \${#aur_pkgs/p' "$ROOT/installer"
    sed -n '/^# Default web browser/,/^friday_default_browser "/p' "$ROOT/installer"
  } | sed "s#/mnt#$W/mnt#g" >"$W/run.sh"
  WORK="$WORK" bash "$W/run.sh"
}

# Chosen browsers: repo ones pacstrapped, AUR ones built as the user and
# installed by pacman; the default is set
browsers "brave firefox zen" zen 2>"$W/err"
log=$(cat "$W/pacstrap.log" 2>/dev/null)
contains "repo browsers pacstrapped" "$log" "pacstrap: firefox"
contains "the AUR build tools come first" "$log" "pacstrap: base-devel git"
contains "brave-bin is built as the user from the AUR" "$log" "build: runuser -u alice -- bash -c"
contains "zen-browser-bin too, built on the disk (not in a /tmp a new chroot empties)" "$log" "/home/alice/.cache/friday-aur/zen-browser-bin zen-browser-bin"
contains "pacman installs what was built" "$log" "chroot: bash -c pacman -U --noconfirm --needed"
contains "default browser set in mimeapps.list" "$(cat "$W/mnt/home/alice/.config/mimeapps.list" 2>/dev/null)" "x-scheme-handler/https=zen.desktop"

# None chosen: nothing installed, Chromium opens links
rm -f "$W/pacstrap.log" "$W/mnt/home/alice/.config/mimeapps.list"
browsers "" chromium
[[ ! -f "$W/pacstrap.log" ]] && pass "nothing is installed when no browsers are chosen" || fail "something was installed when none chosen"
contains "default browser is chromium when none chosen" "$(cat "$W/mnt/home/alice/.config/mimeapps.list" 2>/dev/null)" "x-scheme-handler/https=chromium.desktop"

# A failed AUR build is skipped with a way out, and the install goes on
# Everyday apps: the picks open their files, the terminal picked comes first
# and Super+E opens the file manager picked.
eh="$W/everyday-home"; rm -rf "$eh"; mkdir -p "$eh/.config/hypr"
cp "$ROOT/../etc/skel/.config/xdg-terminals.list" "$eh/.config/"
cp "$ROOT/../etc/skel/.config/hypr/bindings.lua" "$eh/.config/hypr/"
( set -euo pipefail
  eval "$(sed -n '/^friday_mime_default() {/,/^}/p;/^friday_everyday_defaults() {/,/^}/p' "$ROOT/installer")"
  FILE_MANAGER=yazi TERMINAL_APP=ghostty VIDEO_PLAYER=vlc IMAGE_VIEWER=none PDF_VIEWER=okular
  friday_everyday_defaults "$eh" )
mime=$(cat "$eh/.config/mimeapps.list" 2>/dev/null)
contains "the file manager picked opens folders" "$mime" "inode/directory=yazi.desktop"
contains "the video player picked opens videos" "$mime" "video/mp4=vlc.desktop"
contains "the PDF viewer picked opens PDFs" "$mime" "application/pdf=okularApplication_pdf.desktop"
if grep -q '^image/' "$eh/.config/mimeapps.list"; then fail "no image viewer means no image default"; else pass "no image viewer means no image default"; fi
[[ "$(grep -v '^#' "$eh/.config/xdg-terminals.list" | head -n1)" == com.mitchellh.ghostty.desktop ]] \
  && pass "the terminal picked comes first" || fail "the terminal picked comes first"
contains "Super+E opens the file manager picked" "$(cat "$eh/.config/hypr/bindings.lua")" 'o.bind("SUPER + E", "Files", "xdg-terminal-exec yazi")'

# Editors and agents picked in setup: repo ones pacstrapped with Node when an
# npm one is picked, AUR ones built like the browsers.
rm -f "$W/pacstrap.log"
EDITORS="zed vscode" AGENTS="opencode claude cline" browsers "" chromium 2>"$W/err"
log=$(cat "$W/pacstrap.log" 2>/dev/null)
contains "repo editors and agents pacstrapped, with Node for npm ones" "$log" "pacstrap: zed opencode nodejs npm"
contains "VS Code built from the AUR" "$log" "visual-studio-code-bin"
contains "Claude Code built from the AUR" "$log" "claude-code"
rm -f "$W/pacstrap.log"
printf 'x=1' >"$W/mnt/home/alice/.config/mimeapps.list"
out=$(FAIL_AUR=zen-browser-bin browsers "brave zen" brave 3>&1 2>&1) && pass "a failed AUR build doesn't stop the install" || fail "a failed AUR build stopped the install"
contains "it says on screen (fd 3) how to get it later" "$out" "yay -S zen-browser-bin"
contains "the other one is still installed" "$out" "Installed brave-bin from the AUR."
contains "mimeapps.list without a final newline still gets the header" "$(cat "$W/mnt/home/alice/.config/mimeapps.list")" $'x=1\n[Default Applications]'

echo "FRIDAY's orb while the installer waits"
orb_frames=$(bash -c 'source "$1"; for k in 0 1 2 3; do friday_orb $k; echo "--"; done' _ "$REPO_ROOT/iso/friday/airootfs/root/friday-tui.sh" 2>/dev/null)
[[ "$(grep -c '^--$' <<<"$orb_frames")" == 4 && "$(grep -vc '^--$' <<<"$orb_frames")" == 28 ]] \
  && pass "four frames of seven lines" || fail "the orb's frames: $(head -3 <<<"$orb_frames")"
LC_ALL=C grep -q '[^ -~]' <<<"$orb_frames" && fail "the orb uses characters the console font lacks" || pass "the orb is plain ASCII, for the console font"
[[ "$(bash -c 'source "$1"; friday_orb 0' _ "$REPO_ROOT/iso/friday/airootfs/root/friday-tui.sh")" != "$(bash -c 'source "$1"; friday_orb 2' _ "$REPO_ROOT/iso/friday/airootfs/root/friday-tui.sh")" ]] \
  && pass "the frames differ (it moves)" || fail "the orb doesn't move"
grep -q 'friday.nomotion' "$REPO_ROOT/iso/friday/airootfs/root/installer" && pass "friday.nomotion on the boot line stops it" || fail "no way to stop the orb"

echo "installer progress and failure screens"
if need gum python3; then
  W="$WORK/progress"; mkdir -p "$W"
  {
    echo 'set -euo pipefail; LOG_FILE='"$W"'/fake.log; TARGET_DISK=/dev/vda HOSTNAME=mybox USERNAME=alice; : > $LOG_FILE'
    sed -n '/^source "\$(dirname/,/^trap .live_status_stop. EXIT/p' "$ROOT/installer" \
      | sed "s#source \"\$(dirname \"\$(readlink -f \"\$0\")\")/friday-tui.sh\"#source $ROOT/friday-tui.sh#"
    echo 'exec 3>&1 4>&2; exec >>"$LOG_FILE" 2>&1'
    sed -n '/^# On any failure/,/^trap on_failure ERR/p' "$ROOT/installer"
    cat <<'EOF'
phase_msg 3 "Downloading and installing packages (the long one)"
live_status_start
for n in 1 2 3 4; do echo "( $n/980) installing pkg-$n" >> $LOG_FILE; sleep 0.5; done
live_status_stop
phase_msg 5 "Creating alice and applying your theme"
false
EOF
  } >"$W/ui.sh"
  out=$(python3 - "$W/ui.sh" <<'EOF'
import os, pty, re, select, sys, time
pid, fd = pty.fork()
if pid == 0:
    os.environ.update(TERM="xterm-256color", FRIDAY_FAST="1")
    os.execv("/bin/bash", ["bash", sys.argv[1]])
out = b""; end = time.time() + 15
while time.time() < end:
    r, _, _ = select.select([fd], [], [], 0.1)
    if r:
        try: out += os.read(fd, 65536)
        except OSError: break
print(re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", out.decode("utf-8", "replace")).replace("\r", "\n"))
EOF
)
  contains "live status shows the current package" "$out" "installing pkg-"
  contains "install continues past the package step" "$out" "Step 5 of 6"
  contains "failure screen shows the log tail" "$out" "Installation stopped"
else skip "progress screens (needs gum + python3)"; fi

finish
