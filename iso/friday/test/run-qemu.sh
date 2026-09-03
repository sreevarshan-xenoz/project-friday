#!/usr/bin/env bash
# FRIDAY ISO QEMU/KVM Test Harness
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

ISO_PATH=""
DISK_SIZE="20G"
MODE="gui"
DISK_PATH="/tmp/friday-test-ssd.qcow2"
OVMF_VARS="/tmp/friday-ovmf-vars.fd"

usage() {
  echo "Usage: $0 [options]"
  echo "Options:"
  echo "  --iso <path>         Path to FRIDAY ISO (default: latest in /tmp/friday-iso-out/)"
  echo "  --disk-size <size>   Virtual SSD size (default: 20G)"
  echo "  --headless, --debug  Run QEMU headless with serial log output"
  echo "  --gui                Run QEMU with graphical window (default)"
  echo "  --reset-disk         Remove existing test qcow2 disk before running"
  echo "  --help               Display this help message"
  exit 0
}

RESET_DISK=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --iso)
      ISO_PATH="$2"
      shift 2
      ;;
    --disk-size)
      DISK_SIZE="$2"
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
echo "          FRIDAY ISO QEMU/KVM TEST HARNESS                  "
echo "============================================================"
echo "ISO Image:   $ISO_PATH"
echo "Target Disk: $DISK_PATH ($DISK_SIZE)"
echo "Execution:   $MODE"
echo "============================================================"

if [[ "$RESET_DISK" == "true" && -f "$DISK_PATH" ]]; then
  echo "Resetting test qcow2 disk..."
  rm -f "$DISK_PATH"
fi

if [[ ! -f "$DISK_PATH" ]]; then
  echo "Creating disposable test qcow2 disk ($DISK_SIZE)..."
  qemu-img create -f qcow2 "$DISK_PATH" "$DISK_SIZE"
fi

# Prepare UEFI variables template
cp -f /usr/share/edk2/x64/OVMF_VARS.4m.fd "$OVMF_VARS"

BOOT_ORDER="d"
VGA_OPT=("-vga" "std")

if [[ "${BOOT_TARGET:-}" == "disk" ]]; then
  BOOT_ORDER="c"
fi

if [[ "${ENABLE_GPU:-}" == "true" ]]; then
  VGA_OPT=("-vga" "virtio" "-display" "default,show-cursor=on")
fi

QEMU_CMD=(
  qemu-system-x86_64
  -enable-kvm
  -m 4G
  -smp 4
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd
  -drive if=pflash,format=raw,file="$OVMF_VARS"
  -drive file="$DISK_PATH",if=virtio,format=qcow2
  -cdrom "$ISO_PATH"
  -boot "order=$BOOT_ORDER"
  "${VGA_OPT[@]}"
)

if [[ "$MODE" == "debug" ]]; then
  echo "Launching QEMU in headless debug mode (serial log)..."
  "${QEMU_CMD[@]}" -nographic -serial mon:stdio
else
  echo "Launching QEMU interactive graphical VM..."
  "${QEMU_CMD[@]}" &
fi
