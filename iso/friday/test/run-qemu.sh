#!/usr/bin/env bash
# FRIDAY ISO QEMU/KVM Test Harness & Debug Harness
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

ISO_PATH=""
DISK_SIZE="25G"
MODE="gui"
DISK_PATH="/tmp/friday-test-ssd.qcow2"
OVMF_VARS="/tmp/friday-ovmf-vars.fd"
SSH_PORT="2222"
MONITOR_SOCK="/tmp/friday-qemu-monitor.sock"
SERIAL_SOCK="/tmp/friday-qemu-serial.sock"
SERIAL_LOG="/tmp/friday-qemu-serial.log"
BOOT_TARGET="cdrom"
RESET_DISK=false
DAEMONIZE=false

usage() {
  echo "Usage: $0 [options]"
  echo "Options:"
  echo "  --iso <path>         Path to FRIDAY ISO (default: latest in /tmp/friday-iso-out/)"
  echo "  --disk <path>        Custom disk path (default: $DISK_PATH)"
  echo "  --disk-size <size>   Virtual SSD size (default: $DISK_SIZE)"
  echo "  --boot <cdrom|disk>  Boot device priority (default: cdrom)"
  echo "  --headless, --debug  Run QEMU headless with serial and monitor control sockets"
  echo "  --gui                Run QEMU with graphical window"
  echo "  --daemon             Run QEMU detached in background"
  echo "  --ssh-port <port>    Host SSH forwarding port (default: 2222)"
  echo "  --reset-disk         Remove existing test qcow2 disk before running"
  echo "  --help               Display this help message"
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iso)
      ISO_PATH="$2"
      shift 2
      ;;
    --disk)
      DISK_PATH="$2"
      shift 2
      ;;
    --disk-size)
      DISK_SIZE="$2"
      shift 2
      ;;
    --boot)
      BOOT_TARGET="$2"
      shift 2
      ;;
    --headless|--debug)
      MODE="debug"
      shift
      ;;
    --gui)
      MODE="gui"
      shift
      ;;
    --daemon)
      DAEMONIZE=true
      shift
      ;;
    --ssh-port)
      SSH_PORT="$2"
      shift 2
      ;;
    --reset-disk)
      RESET_DISK=true
      shift
      ;;
    --help)
      usage
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      ;;
  esac
done

# Find latest ISO if not specified
if [[ -z "$ISO_PATH" ]]; then
  ISO_PATH=$(ls -t /tmp/friday-iso-out/*.iso 2>/dev/null | head -n 1 || true)
fi

if [[ -z "$ISO_PATH" || ! -f "$ISO_PATH" ]]; then
  echo "Error: No ISO image found. Please build the ISO first or specify --iso." >&2
  exit 1
fi

echo "============================================================"
echo "          FRIDAY ISO QEMU/KVM TEST & DEBUG HARNESS          "
echo "============================================================"
echo "ISO Image:     $ISO_PATH"
echo "Target Disk:   $DISK_PATH ($DISK_SIZE)"
echo "Execution:     $MODE"
echo "Boot Target:   $BOOT_TARGET"
echo "SSH Port:      127.0.0.1:$SSH_PORT -> Guest 22"
echo "Monitor Sock:  $MONITOR_SOCK"
echo "Serial Sock:   $SERIAL_SOCK"
echo "Serial Log:    $SERIAL_LOG"
echo "============================================================"

# Clean existing sockets
rm -f "$MONITOR_SOCK" "$SERIAL_SOCK"

if [[ "$RESET_DISK" == "true" && -f "$DISK_PATH" ]]; then
  echo "Resetting test qcow2 disk..."
  rm -f "$DISK_PATH"
fi

if [[ ! -f "$DISK_PATH" ]]; then
  echo "Creating disposable test qcow2 disk ($DISK_SIZE)..."
  qemu-img create -f qcow2 "$DISK_PATH" "$DISK_SIZE"
fi

# Prepare UEFI variables template if missing or resetting
if [[ ! -f "$OVMF_VARS" || "$RESET_DISK" == "true" ]]; then
  cp -f /usr/share/edk2/x64/OVMF_VARS.4m.fd "$OVMF_VARS"
fi

BOOT_ORDER="d"
if [[ "$BOOT_TARGET" == "disk" ]]; then
  BOOT_ORDER="c"
fi

VGA_OPT=("-vga" "virtio")
if [[ "$MODE" == "gui" ]]; then
  VGA_OPT=("-vga" "virtio" "-display" "default,show-cursor=on")
fi

QEMU_CMD=(
  qemu-system-x86_64
  -enable-kvm
  -cpu host
  -m 6G
  -smp 4
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd
  -drive if=pflash,format=raw,file="$OVMF_VARS"
  -drive file="$DISK_PATH",if=virtio,format=qcow2,cache=unsafe,aio=threads
  -cdrom "$ISO_PATH"
  -boot "order=$BOOT_ORDER"
  -netdev "user,id=net0,hostfwd=tcp::$SSH_PORT-:22"
  -device virtio-net-pci,netdev=net0
  -chardev "socket,id=mon0,path=$MONITOR_SOCK,server=on,wait=off"
  -mon chardev=mon0,mode=readline
  "${VGA_OPT[@]}"
)

if [[ "$MODE" == "debug" ]]; then
  # In debug mode, connect serial port to unix socket and mirror to log file
  QEMU_CMD+=(
    -nographic
    -chardev "socket,id=ser0,path=$SERIAL_SOCK,server=on,wait=off,logfile=$SERIAL_LOG,logappend=on"
    -serial chardev:ser0
  )
  touch "$SERIAL_LOG"
  echo "Starting QEMU debug VM..."
  if [[ "$DAEMONIZE" == "true" ]]; then
    nohup "${QEMU_CMD[@]}" >/tmp/friday-qemu-stdout.log 2>/tmp/friday-qemu-stderr.log < /dev/null &
    QEMU_PID=$!
    disown "$QEMU_PID" 2>/dev/null || true
    echo "QEMU running in background with PID $QEMU_PID"
  else
    # Run in background and tail serial log live
    "${QEMU_CMD[@]}" &
    QEMU_PID=$!
    echo "QEMU PID: $QEMU_PID. Streaming live serial log (Ctrl+C to stop stream; VM remains running)..."
    tail -f "$SERIAL_LOG" --pid="$QEMU_PID" || true
  fi
else
  echo "Launching QEMU interactive graphical VM..."
  "${QEMU_CMD[@]}" &
fi
