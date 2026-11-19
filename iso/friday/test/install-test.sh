#!/usr/bin/env bash
# Install a FRIDAY OS ISO in QEMU and check the installed desktop.
#
#   bash iso/friday/test/install-test.sh <friday-os.iso> [out-dir]
#
# Boots the ISO (UEFI, KVM when available), runs the installer unattended
# over SSH, boots the installed disk with autologin, then checks what a person
# would hit first: the hostname, the FRIDAY daemon, Hyprland's config
# (deprecations, errors), apps that fail to start, the commands Omarchy's
# bindings run, the `friday` CLI and `friday doctor`. The ISO downloads the
# desktop from the Arch mirrors at install time, so this is what catches an
# Arch update breaking an install (CI runs it weekly).
#
# Leaves a screenshot of the desktop, the installer log and the session
# journal in out-dir. Needs: qemu-system-x86_64, OVMF (edk2), ssh,
# qemu-img, python3. Exit status: 0 when every check passes.
set -euo pipefail

ISO="${1:?usage: install-test.sh <iso> [out-dir]}"
OUT="${2:-$PWD/install-test-out}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
# VM files beside the output: the disk grows to several GB (too big for a RAM /tmp).
WORK="$(mktemp -d -p "$OUT" .vm.XXXXXX)"
PORT="${INSTALL_TEST_SSH_PORT:-2229}"
MON="$WORK/monitor.sock"
PASS_WORD=friday
QEMU_PID=""

log() { echo "[install-test] $*"; }
cleanup() {
  [[ -n "$QEMU_PID" ]] && kill "$QEMU_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

for d in /usr/share/edk2/x64 /usr/share/OVMF /usr/share/edk2-ovmf/x64; do
  for c in OVMF_CODE.4m.fd OVMF_CODE_4M.fd; do
    [[ -f "$d/$c" ]] && { OVMF_CODE="$d/$c"; OVMF_VARS_SRC="$d/${c/CODE/VARS}"; break 2; }
  done
done
[[ -n "${OVMF_CODE:-}" && -f "${OVMF_VARS_SRC:-}" ]] || { echo "OVMF (edk2) not found" >&2; exit 2; }
cp "$OVMF_VARS_SRC" "$WORK/vars.fd"
qemu-img create -q -f qcow2 "$WORK/disk.qcow2" 25G
ACCEL=(-accel "tcg,thread=multi" -cpu max)
[[ -w /dev/kvm ]] && ACCEL=(-enable-kvm -cpu host)

boot() { # boot <cdrom|disk>
  local media=(-cdrom "$ISO" -boot order=d)
  [[ "$1" == disk ]] && media=(-boot order=c)
  rm -f "$MON"
  qemu-system-x86_64 "${ACCEL[@]}" -m 4G -smp 4 \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$WORK/vars.fd" \
    -drive file="$WORK/disk.qcow2",if=virtio,format=qcow2,cache=unsafe \
    "${media[@]}" \
    -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:$PORT-:22" -device virtio-net-pci,netdev=net0 \
    -device virtio-vga -display none \
    -chardev "socket,id=mon0,path=$MON,server=on,wait=off" -mon chardev=mon0,mode=readline \
    -serial "file:$OUT/serial-$1.log" >"$OUT/qemu-$1.log" 2>&1 &
  QEMU_PID=$!
}

monitor() { # monitor <command>: one HMP command on the QEMU monitor
  python3 - "$MON" "$1" <<'EOF'
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.settimeout(10); s.connect(sys.argv[1])
time.sleep(0.2); s.recv(65536); s.sendall((sys.argv[2] + "\n").encode()); time.sleep(1)
EOF
}

stop_vm() {
  monitor quit 2>/dev/null || true
  for _ in $(seq 1 30); do kill -0 "$QEMU_PID" 2>/dev/null || break; sleep 1; done
  kill "$QEMU_PID" 2>/dev/null || true
  QEMU_PID=""
}

# SSH without sshpass: the password comes from an askpass helper.
printf '#!/bin/sh\necho %s\n' "$PASS_WORD" >"$WORK/askpass"
chmod +x "$WORK/askpass"
vm() { # vm <user> <command...>
  local user="$1"; shift
  SSH_ASKPASS="$WORK/askpass" SSH_ASKPASS_REQUIRE=force DISPLAY=none \
    ssh -p "$PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR -o ConnectTimeout=10 -o PreferredAuthentications=password,keyboard-interactive,none \
    "$user@127.0.0.1" "$@"
}

wait_ssh() { # wait_ssh <user> <seconds>: QEMU's forward accepts TCP before the guest is up, so run a real command
  local deadline=$((SECONDS + $2))
  until vm "$1" true 2>/dev/null; do
    (( SECONDS < deadline )) || { log "no SSH as $1 after $2 s"; return 1; }
    sleep 5
  done
}

log "booting the ISO"
boot cdrom
wait_ssh root 300
log "live system is up; installing"
# Through env: the live shell is zsh, where USERNAME (and HOSTNAME in bash) are
# special parameters that a plain prefix assignment doesn't pass on.
if ! vm root "env TERM=xterm TARGET_DISK=/dev/vda ERASE_DISK=/dev/vda FRIDAY_HOSTNAME=friday USERNAME=friday PASSWORD=$PASS_WORD \
      AUTOLOGIN=true ENCRYPT=false FRIDAY_DEBUG_ACCESS=true AUTO_REBOOT=false FRIDAY_FAST=1 \
      timeout 3000 /root/installer" >"$OUT/installer.log" 2>&1; then
  log "the installer failed; last lines:"; tail -n 30 "$OUT/installer.log" | sed 's/\x1b\[[0-9;]*[a-zA-Z]//g'
  exit 1
fi
log "installed; booting the disk"
vm root 'sync' || true
stop_vm
boot disk
wait_ssh friday 300

# Autologin starts Hyprland; give the session's autostart time to settle.
session_env='export XDG_RUNTIME_DIR=/run/user/$(id -u); export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus;
  export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr 2>/dev/null | head -n1);
  export WAYLAND_DISPLAY=$(cd $XDG_RUNTIME_DIR && ls wayland-? 2>/dev/null | head -n1)'
for _ in $(seq 1 60); do
  vm friday "$session_env; [ -n \"\$HYPRLAND_INSTANCE_SIGNATURE\" ] && hyprctl version >/dev/null 2>&1" && break
  sleep 5
done
sleep 30

FAILS=0
check() { # check <description> <remote command>: pass when the command exits 0
  if out=$(vm friday "$session_env; $2" 2>&1); then
    echo "  ok   $1"
  else
    echo "  FAIL $1"; echo "$out" | head -n 15 | sed 's/^/       /'; FAILS=$((FAILS + 1))
  fi
}

echo "installed system"
check "computer is named friday" '[ "$(cat /etc/hostname)" = friday ]'
check "Hyprland session is running" 'hyprctl version >/dev/null'
check "Hyprland reports no deprecated options" 'out=$(hyprctl deprecated-config); echo "$out"; echo "$out" | grep -qi "all good"'
check "Hyprland reports no config errors" 'out=$(hyprctl configerrors); echo "$out"; [ -z "$(echo "$out" | tr -d "[:space:]")" ]'
# Lua binds show an internal id as `arg`, so match on the bind descriptions.
check "FRIDAY OS keymap is live, Omarchy's defaults off" 'hyprctl binds -j | jq -e "any(.[]; .modmask == 64 and .key == \"Y\" and .description == \"FRIDAY: Ask\") and any(.[]; .key == \"Super_L\" and .release) and (any(.[]; .description == \"Pop window out (float & pin)\") | not)"'
check "FRIDAY daemon is active" 'systemctl --user is-active friday-linux-daemon'
check "FRIDAY Shell is running" 'ps -eo args | grep -q "^quickshell .*/usr/share/friday/shell"'
check "no app failed to start in the session" '! journalctl --user -b --no-pager | grep -E "Command not found|App failure|binary could not be found"'
check "commands the desktop runs are installed" 'm=""; for c in xdg-terminal-exec udiskie inotifywait nautilus hyprpicker wpctl brightnessctl grim slurp hyprlock fuzzel wl-paste notify-send playerctl jq btop xdg-user-dir powerprofilesctl; do command -v $c >/dev/null || m="$m $c"; done; [ -z "$m" ] || { echo "missing:$m"; false; }'
check "omarchy-* commands are on a login shell's PATH" 'bash -lc "command -v omarchy-launch-terminal omarchy-version-channel" >/dev/null'
check "friday in zsh is the FRIDAY CLI, not an alias" 'w=$(zsh -c "source ~/.zshrc >/dev/null 2>&1; whence -w friday"); echo "$w"; [ "$w" = "friday: command" ]'
check "friday doctor finds no problems" 'friday doctor'

# The lock screen locks, takes the password typed on the VM's keyboard, and
# locks again after that (a stale lock state once made the second lock a no-op).
lock_state() { vm friday "$session_env; bash -lc 'omarchy-shell lock isLocked'" 2>/dev/null; }
lock_round() {
  vm friday "$session_env; bash -lc 'omarchy-shell lock lock'" >/dev/null 2>&1; sleep 3
  [[ "$(lock_state)" == true ]] || { echo "did not lock"; return 1; }
  for k in f r i d a y ret; do monitor "sendkey $k 60" >/dev/null; sleep 0.2; done
  sleep 6
  [[ "$(lock_state)" == false ]] || { echo "the password did not unlock it"; return 1; }
}
if out=$(vm friday 'test -f /etc/pam.d/omarchy-lock-password' 2>&1); then echo "  ok   the lock screen has its PAM service"
else echo "  FAIL the lock screen has no /etc/pam.d/omarchy-lock-password"; FAILS=$((FAILS + 1)); fi
for round in 1 2; do
  if out=$(lock_round 2>&1); then echo "  ok   lock screen round $round: locks, then the password unlocks it"
  else echo "  FAIL lock screen round $round: $out"; FAILS=$((FAILS + 1)); fi
done

log "saving a screenshot and logs"
monitor "screendump $WORK/desktop.ppm" || true
sleep 2
if [[ -f "$WORK/desktop.ppm" ]]; then
  python3 -c "from PIL import Image; Image.open('$WORK/desktop.ppm').save('$OUT/desktop.png')" 2>/dev/null \
    || cp "$WORK/desktop.ppm" "$OUT/desktop.ppm"
fi
vm friday 'journalctl --user -b --no-pager' >"$OUT/session-journal.log" 2>&1 || true
vm friday "$session_env; hyprctl deprecated-config; hyprctl configerrors" >"$OUT/hyprland.log" 2>&1 || true
stop_vm

if (( FAILS )); then log "$FAILS check(s) failed"; exit 1; fi
log "all checks passed"
