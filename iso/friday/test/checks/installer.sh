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
expect "banner and rail work without TERM" env -u TERM bash -c \
  "source '$ROOT/friday-tui.sh'; set -e; friday_banner TEST; friday_rail 2 A B C; friday_say hi" </dev/null

echo "login security"
guarded=$(awk '/if \[\[ "\$AUTOLOGIN" == "true" \]\]/ {g=1} /EOF_SDDM_AUTO > \/mnt\/etc\/sddm.conf.d\/autologin.conf/ {print (g ? "guarded" : "unguarded"); exit}' "$ROOT/installer")
if [[ "$guarded" == guarded ]]; then pass "SDDM autologin is only written when AUTOLOGIN=true"
else fail "installer writes SDDM autologin unconditionally"; fi
expect "ESP mounted root-only (fmask/dmask 0077)" grep -q 'mount -o fmask=0077,dmask=0077 "$BOOT_PART" /mnt/boot' "$ROOT/installer"
expect "AUTOLOGIN defaults to false" grep -q 'AUTOLOGIN="${AUTOLOGIN:-false}"' "$ROOT/installer"

echo "configurator flow (pty)"
if need gum python3; then
  W="$WORK/installer"; mkdir -p "$W/stub"
  cp "$ROOT/configurator" "$ROOT/friday-tui.sh" "$W/"
  sed -i "s#/root/installer#$W/stub/installer#" "$W/configurator"
  cat >"$W/stub/lsblk" <<'EOF'
#!/bin/bash
if [[ "$*" == *"-d -n -o NAME,TYPE,SIZE,MODEL"* ]]; then echo "vda disk 25G QEMU_HARDDISK"; else echo ext4; fi
EOF
  printf '#!/bin/bash\nexit 1\n' >"$W/stub/findmnt"
  printf '#!/bin/bash\nexit 0\n' >"$W/stub/curl"   # pretend we're online
  cat >"$W/stub/installer" <<'EOF'
#!/bin/bash
echo "INSTALLER-ENV disk=$TARGET_DISK host=$HOSTNAME user=$USERNAME pw=$PASSWORD tz=$TIMEZONE kb=$KEYBOARD ai=[$FRIDAY_AI_PROVIDER] autologin=$AUTOLOGIN encrypt=$ENCRYPT interactive=$FRIDAY_INTERACTIVE session=$SESSION style=$SHELL_STYLE"
EOF
  chmod +x "$W"/stub/* "$W/configurator"
  if python3 "$CHECKS/installer_flow.py" "$W/configurator" "$W/stub"; then :; else FAILS=$((FAILS + 1)); fi
else skip "configurator flow (needs gum + python3)"; fi

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
