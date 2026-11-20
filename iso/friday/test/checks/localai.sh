#!/usr/bin/env bash
# `friday local-ai`: hardware detection (PCI display devices), budget presets and the model each
# gets, on fake /proc and /sys trees (FRIDAY_HW_ROOT), and the install with
# its downloads and services stubbed.
source "$(dirname "$0")/lib.sh"

F="$REPO_ROOT/packaging/linux/friday"
W="$WORK/localai"; S="$W/stub"
rm -rf "$W"; mkdir -p "$S" "$W/home"
export HOME="$W/home" XDG_CONFIG_HOME="" XDG_STATE_HOME="" PATH="$S:$PATH"

# machine <name> <ram_gb> <avx2 yes|no> [vendor [vram_mb]]: a fake hardware tree.
machine() {
  local r="$W/hw/$1"
  mkdir -p "$r/proc" "$r/sys/bus/pci/devices/0000:00:1f.0"
  echo 0x060100 >"$r/sys/bus/pci/devices/0000:00:1f.0/class"  # not a display device
  echo 0x8086 >"$r/sys/bus/pci/devices/0000:00:1f.0/vendor"
  printf 'MemTotal:       %d kB\nMemAvailable:   %d kB\n' $(($2 * 1024 * 1024)) $(($2 * 512 * 1024)) >"$r/proc/meminfo"
  local flags="fpu sse sse2"; [[ $3 == yes ]] && flags="$flags avx avx2"
  for i in 0 1 2 3 4 5 6 7; do printf 'processor\t: %d\nflags\t\t: %s\n\n' "$i" "$flags"; done >"$r/proc/cpuinfo"
  if [[ -n "${4:-}" ]]; then
    local dev="$r/sys/bus/pci/devices/0000:01:00.0"
    mkdir -p "$dev"
    echo 0x030000 >"$dev/class"
    echo "$4" >"$dev/vendor"
    [[ -n "${5:-}" ]] && echo $(($5 * 1024 * 1024)) >"$dev/mem_info_vram_total"
  fi
  echo "$r"
}
presets() { FRIDAY_HW_ROOT="$1" FRIDAY_DISK_FREE_MB="${2:-100000}" bash "$F" local-ai presets; }
# field <presets output> <preset id> <field number>
field() { awk -F'|' -v id="$2" -v n="$3" '$1 == id { print $n }' <<<"$1"; }

echo "detection"
hw=$(machine amd16 16 yes 0x1002 8192)
d=$(FRIDAY_HW_ROOT="$hw" FRIDAY_DISK_FREE_MB=50000 bash "$F" local-ai detect)
contains "RAM in MB" "$d" "ram_mb=16384"
contains "an AMD GPU" "$d" "gpu=amd"
contains "AMD VRAM from sysfs" "$d" "vram_mb=8192"
contains "AMD with its own VRAM is discrete" "$d" "gpu_discrete=yes"
contains "CPU threads" "$d" "cpu_threads=8"
contains "AVX2" "$d" "avx2=yes"
contains "free disk" "$d" "disk_free_mb=50000"
hw_nv=$(machine nvidia16 16 yes 0x10de)
d=$(FRIDAY_HW_ROOT="$hw_nv" bash "$F" local-ai detect)
contains "NVIDIA without its driver: VRAM unknown" "$d" "vram_mb=unknown"
printf '#!/bin/sh\necho "NVIDIA-SMI has failed" >&2; exit 9\n' >"$S/nvidia-smi"; chmod +x "$S/nvidia-smi"
d=$(FRIDAY_HW_ROOT="$hw_nv" bash "$F" local-ai detect)
contains "nvidia-smi failing (no driver) is not an error" "$d" "vram_mb=unknown"
printf '#!/bin/sh\necho 4096\n' >"$S/nvidia-smi"
d=$(FRIDAY_HW_ROOT="$hw_nv" bash "$F" local-ai detect)
contains "NVIDIA VRAM from nvidia-smi" "$d" "vram_mb=4096"
rm "$S/nvidia-smi"
hw_none=$(machine none8 8 yes)
contains "no GPU (a non-display PCI device doesn't count)" "$(FRIDAY_HW_ROOT="$hw_none" bash "$F" local-ai detect)" "gpu=none"

echo "presets"
p=$(presets "$hw")
[[ "$(field "$p" gpu 4)" == gemma3:4b ]] && pass "8 GB VRAM: the GPU preset runs gemma3:4b" || fail "GPU preset: $(field "$p" gpu 4)"
[[ "$(field "$p" gpu 7)" == 1 ]] && pass "the GPU preset is fast" || fail "GPU preset not marked fast"
[[ "$(field "$p" most 4)" == gemma3:12b ]] && pass "16 GB, Most (12 GB): gemma3:12b" || fail "Most: $(field "$p" most 4)"
[[ "$(field "$p" most 6)" == 1 ]] && pass "gemma3:12b sees screenshots" || fail "gemma3:12b not vision"
[[ "$(field "$p" light 4)" == qwen3:4b && "$(field "$p" light 6)" == 0 ]] && pass "Light is text only (qwen3:4b)" || fail "Light: $(field "$p" light 4) vision=$(field "$p" light 6)"

hw_small=$(machine small2 2 yes)
p=$(presets "$hw_small")
[[ -z "$(field "$p" balanced 4)" ]] && pass "2 GB, Balanced (1 GB): no model" || fail "2 GB Balanced got $(field "$p" balanced 4)"
contains "too little memory is said in words" "$(field "$p" balanced 9)" "too little memory"
[[ "$(field "$p" most 4)" == "" ]] && pass "2 GB, Most (1.5 GB): still too little" || fail "2 GB Most got $(field "$p" most 4)"

p=$(presets "$hw_nv")
[[ -z "$(field "$p" gpu 1)" ]] && pass "NVIDIA, VRAM unknown: no GPU preset" || fail "a GPU preset without known VRAM"
contains "and the summary says why" "$(FRIDAY_HW_ROOT="$hw_nv" bash "$F" local-ai summary)" "NVIDIA driver is installed"

p=$(presets "$hw" 1500)
[[ "$(field "$p" most 8)" == 0 ]] && pass "not enough disk: the preset is unavailable" || fail "Most available with 1.5 GB free"
contains "and says why" "$(field "$p" most 9)" "disk"

hw_old=$(machine old8 8 no)
contains "no AVX2 and no GPU: very slow" "$(FRIDAY_HW_ROOT="$hw_old" bash "$F" local-ai summary)" "very slow"
contains "summary: memory, GPU and its memory, threads" "$(FRIDAY_HW_ROOT="$hw" bash "$F" local-ai summary)" "16 GB memory, AMD GPU 8 GB, 8 threads"

finish
