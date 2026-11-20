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

echo "install"
LOG="$W/calls.log"; : >"$LOG"
for c in ollama systemctl notify-send; do
  printf '#!/bin/sh\necho "%s $*" >>"%s"\n' "$c" "$LOG" >"$S/$c"
done
# curl: health checks succeed; a download writes a file that isn't the real one.
cat >"$S/curl" <<STUB
#!/bin/bash
echo "curl \$*" >>"$LOG"
out=""; while (( \$# )); do [[ \$1 == -o ]] && { out=\$2; shift; }; shift; done
[[ -n "\$out" ]] && echo "not the model" >"\$out"
exit 0
STUB
chmod +x "$S"/*
CONF="$HOME/.config/friday/local-ai.conf" ENV="$HOME/.config/friday/friday.env"
mkdir -p "$HOME/.config/friday"
printf 'engine=ollama\nbudget_mb=6144\nstate=pending\n' >"$CONF"
(umask 077; printf 'GEMINI_API_KEY=k\nLLAMACPP_MODEL=old\n' >"$ENV")
if FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --first-login >"$W/out" 2>&1; then pass "Ollama install succeeds"
else fail "Ollama install failed: $(tail -3 "$W/out")"; fi
contains "pulls the model the budget fits (6 GB: gemma3:4b)" "$(cat "$LOG")" "ollama pull gemma3:4b"
envf=$(cat "$ENV")
contains "keeps the other keys" "$envf" "GEMINI_API_KEY=k"
contains "points FRIDAY at Ollama" "$envf" "LLAMACPP_BASE_URL=http://127.0.0.1:11434/v1"
contains "names the model" "$envf" "LLAMACPP_MODEL=gemma3:4b"
contains "it sees screenshots" "$envf" "LLAMACPP_VISION=1"
[[ $(grep -c '^LLAMACPP_MODEL=' "$ENV") == 1 ]] && pass "replaces the old model line" || fail "two LLAMACPP_MODEL lines"
[[ "$(stat -c %a "$ENV")" == 600 ]] && pass "friday.env stays private (0600)" || fail "friday.env is $(stat -c %a "$ENV")"
contains "state is ready" "$(cat "$CONF")" "state=ready"
contains "restarts the daemon" "$(cat "$LOG")" "systemctl --user restart friday-linux-daemon"
contains "says local AI is on, and that it sees screenshots" "$(cat "$LOG")" "sees screenshots"
: >"$LOG"
FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --first-login >/dev/null 2>&1
[[ ! -s "$LOG" ]] && pass "first login with the model ready does nothing" || fail "first login re-ran: $(head -2 "$LOG")"
contains "status names the model" "$(bash "$F" local-ai status)" "gemma3:4b"

: >"$LOG"; cp "$ENV" "$W/env.before"
printf 'engine=llamacpp\nbudget_mb=3000\nstate=pending\n' >"$CONF"
if FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --first-login >"$W/out" 2>&1; then fail "a bad download counted as installed"
else pass "llama.cpp: a download that fails its checksum fails the install"; fi
contains "state is failed" "$(cat "$CONF")" "state=failed"
contains "the reason names the checksum" "$(cat "$CONF")" "checksum"
[[ "$(cat "$ENV")" == "$(cat "$W/env.before")" ]] && pass "friday.env is untouched" || fail "friday.env changed after a failed install"
contains "the failure is a notification" "$(cat "$LOG")" "notify-send"
contains "llama.cpp: the GPU build for a machine with a GPU" "$(cat "$LOG")" "ubuntu-vulkan-x64"

: >"$LOG"
printf 'engine=ollama\nbudget_mb=6144\nstate=installing\n' >"$CONF"
FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --first-login >/dev/null 2>&1
contains "a download a reboot cut short resumes at the next login" "$(cat "$LOG")" "ollama pull gemma3:4b"
rm -f "$CONF"; : >"$LOG"
if FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --engine ollama --budget 4096 >"$W/out" 2>&1; then pass "install with no settings file yet works"
else fail "install with no settings file failed: $(tail -2 "$W/out")"; fi

printf 'engine=ollama\nbudget_mb=1000\nstate=pending\n' >"$CONF"
FRIDAY_HW_ROOT="$hw" bash "$F" local-ai install --first-login >/dev/null 2>&1
contains "too little memory: failed, in words" "$(cat "$CONF")" "too little memory"

echo "packages per GPU"
[[ "$(FRIDAY_HW_ROOT="$hw_nv" bash "$F" local-ai packages ollama)" == ollama-cuda ]] && pass "NVIDIA: ollama-cuda" || fail "NVIDIA Ollama package"
[[ "$(FRIDAY_HW_ROOT="$hw" bash "$F" local-ai packages ollama)" == ollama-vulkan ]] && pass "AMD: ollama-vulkan" || fail "AMD Ollama package"
[[ "$(FRIDAY_HW_ROOT="$hw_none" bash "$F" local-ai packages ollama)" == ollama ]] && pass "no GPU: ollama" || fail "CPU Ollama package"
[[ "$(FRIDAY_HW_ROOT="$hw" bash "$F" local-ai packages llamacpp)" == "vulkan-icd-loader vulkan-radeon" ]] && pass "llama.cpp on AMD: Vulkan" || fail "llama.cpp AMD packages"
[[ -z "$(FRIDAY_HW_ROOT="$hw_none" bash "$F" local-ai packages llamacpp)" ]] && pass "llama.cpp with no GPU: nothing extra" || fail "llama.cpp CPU packages"

echo "setup-ai local"
printf '#!/bin/sh\necho "sudo $*" >>"%s"\n' "$LOG" >"$S/sudo"; chmod +x "$S/sudo"
rm -f "$CONF"; : >"$LOG"
if FRIDAY_HW_ROOT="$hw" bash "$F" setup-ai local --engine ollama --preset balanced >"$W/out" 2>&1; then pass "setup-ai local with flags succeeds"
else fail "setup-ai local failed: $(tail -3 "$W/out")"; fi
contains "describes the computer" "$(cat "$W/out")" "This computer: 16 GB memory, AMD GPU 8 GB"
contains "Balanced on 16 GB pulls gemma3:4b" "$(cat "$LOG")" "ollama pull gemma3:4b"
contains "Ollama already installed: no pacman" "$(grep -c pacman "$LOG" || true)" "0"
contains "starts Ollama's service" "$(cat "$LOG")" "sudo systemctl enable --now ollama"
contains "the choice is saved" "$(cat "$CONF")" "budget_mb=8192"
refuse "an unknown preset is refused" env FRIDAY_HW_ROOT="$hw_nv" bash "$F" setup-ai local --engine ollama --preset gpu

finish
