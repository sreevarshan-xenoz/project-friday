# Local AI and several providers at install: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The installer takes several AI providers plus an optional local model sized to the machine; the model downloads at first login and answers first.

**Architecture:** All local-AI logic lives in the `friday` CLI (`packaging/linux/friday`, shipped in `friday-git`, which is also on the live ISO): `friday local-ai detect|presets|install|status`. The ISO configurator calls it to show real numbers and saves the choice; the installer writes keys and the choice into the target and enables a per-user oneshot unit that runs `friday local-ai install` at first login.

**Tech Stack:** bash (CLI, installer, configurator, gum), systemd user units, Ollama (Arch `ollama`, `ollama-cuda`, `ollama-vulkan`), llama.cpp release build b12556, Python pty test.

**Spec:** `docs/superpowers/specs/2026-11-20-local-ai-setup-design.md`

## Global Constraints

- Ladder (fits-in budget → model): <2 GB none; 2 GB qwen3:1.7b text; 3.5 GB qwen3:4b text; 4.5 GB gemma3:4b vision; 9 GB gemma3:12b vision; 19 GB gemma3:27b vision.
- Presets: Light 25% RAM, Balanced 50%, Most 75%, GPU = VRAM − 512 MB (discrete GPU with known VRAM only).
- Ollama package: NVIDIA → `ollama-cuda`; other GPU → `ollama-vulkan`; none → `ollama`. Pacstrapped by the installer (first login has no root).
- llama.cpp: `llama-b12556-bin-ubuntu-vulkan-x64.tar.gz` (GPU) / `llama-b12556-bin-ubuntu-x64.tar.gz` (CPU), SHA-256 checked, unpacked to `~/.local/share/friday/llama.cpp`, served by `friday-llama.service` on 127.0.0.1:8080.
- GGUFs pinned by Hugging Face revision and SHA-256 (Q4_K_M; Qwen3-1.7B Q8_0; gemma `mmproj-model-f16.gguf` for vision).
- Choice file `~/.config/friday/local-ai.conf`: `engine=ollama|llamacpp`, `budget_mb=N`, `model=<ladder id>`, `state=pending|installing|ready|failed`.
- Keys and `LLAMACPP_*` go in `~/.config/friday/friday.env` (0600), never echoed.
- Screen wording says "text only" / "sees screenshots" and the too-small case in words.

## Review Focus

1. NVIDIA without its driver (VRAM unknown) → no GPU preset, RAM presets, the summary says why. Test: fake PCI tree with NVIDIA, failing `nvidia-smi`.
2. A preset whose download exceeds free disk → shown as unavailable, not selectable. Test: `disk_free_mb=1500`.
3. Local chosen, model not ready, no cloud → the ask says it's still downloading. Test in Task 5 (CLI ask path reads `state=installing`).
4. Download fails or checksum mismatch → `state=failed`, notification with the reason, cloud keeps answering; re-running `friday local-ai install` retries. Test: stubbed `curl` returning a wrong file.
5. Several keyed providers, one key empty → that provider is skipped with a note, the others are saved. Test in the flow test.

---

### Task 1: `friday local-ai detect` and `presets`

**Files:** Modify `packaging/linux/friday` (new `friday_local_ai` + dispatch `local-ai)`); Test: new `iso/friday/test/checks/local_ai.sh`, wired into `run-checks.sh` under the `installer` group.

**Produces:**
- `friday local-ai detect` → lines `ram_mb= gpu=nvidia|amd|intel|none gpu_discrete=yes|no vram_mb=N|unknown cpu_threads= avx2=yes|no disk_free_mb=`. Reads `${FRIDAY_HW_ROOT:-}/proc/meminfo`, `/proc/cpuinfo`, `/sys/class/drm/card*/device/{vendor,mem_info_vram_total}`; `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits` when on PATH; `df -Pm "${FRIDAY_DISK_PATH:-$HOME}"`. Overrides for tests: `FRIDAY_HW_ROOT`, `FRIDAY_DISK_FREE_MB`.
- `friday local-ai presets [--json]` → one line per preset `id|label|budget_mb|model|download_mb|vision(0|1)|fast(0|1)|available(0|1)|note`. Vendor ids: 0x10de NVIDIA, 0x1002 AMD, 0x8086 Intel; AMD with `mem_info_vram_total` ≥ 2 GB counts discrete.
- `friday_local_ai_model <budget_mb>` → ladder id or empty.

- [ ] Write `local_ai.sh` with fake trees: 2 GB RAM no GPU → Light/Balanced/Most all "too little" except Most=qwen3:1.7b text; 16 GB + AMD 8 GB VRAM → GPU preset gemma3:4b fast, Most gemma3:12b slow; NVIDIA no nvidia-smi → no GPU preset, note mentions first login; `FRIDAY_DISK_FREE_MB=1500` → gemma presets available=0; no avx2 + no GPU → note "very slow".
- [ ] Run `bash iso/friday/test/run-checks.sh installer` — fails (no command).
- [ ] Implement; run again — passes. `shellcheck` clean on the new functions.
- [ ] Commit `feat(cli): friday local-ai detect and presets`.

### Task 2: `friday local-ai install|status` (Ollama and llama.cpp)

**Files:** Modify `packaging/linux/friday`; Create `packaging/linux/friday-local-ai.service`, `packaging/linux/friday-llama.service`; Modify `packaging/arch/PKGBUILD` (install both units to `/usr/lib/systemd/user`); Test: `local_ai.sh`.

**Consumes:** Task 1's detect/model functions. **Produces:** `friday local-ai install [--engine E] [--budget MB]` (defaults from `local-ai.conf`), `friday local-ai status`, `friday local-ai off`.

- Ollama: wait for `127.0.0.1:11434`, `ollama pull <tag>`. llama.cpp: download tarball + GGUF (+mmproj) with `curl -fL`, `sha256sum -c`, unpack, write `~/.config/friday/llama.env` (`MODEL=`, `MMPROJ=`), `systemctl --user enable --now friday-llama.service`.
- Then writes `LLAMACPP_ENABLED=1`, `LLAMACPP_BASE_URL` (11434/v1 or 8080/v1), `LLAMACPP_MODEL`, `LLAMACPP_VISION` to `friday.env` (replacing old lines), sets `state=ready`, `systemctl --user restart friday-linux-daemon`, `notify-send "Local AI is on" "<model> (text only|sees screenshots)"`. Any failure → `state=failed` + `reason=` + notification; exit 1.
- `friday-local-ai.service`: `Type=oneshot`, `ConditionPathExists=%h/.config/friday/local-ai.conf`, `ExecStart=/usr/bin/friday local-ai install --first-login` (`--first-login` exits 0 unless `state=pending|failed`).
- `friday-llama.service`: `EnvironmentFile=%h/.config/friday/llama.env`, `ExecStart=%h/.local/share/friday/llama.cpp/llama-server -m ${MODEL} $MMPROJ_ARG --host 127.0.0.1 --port 8080`, `Restart=on-failure`.
- `friday private on` uses `friday_local_ai_model` on the Balanced budget instead of its RAM guess.

- [ ] Tests (stub `ollama`, `curl`, `systemctl`, `notify-send`, `sha256sum` real): Ollama path pulls `gemma3:4b` for a 6 GB budget and writes the four `LLAMACPP_*` lines; llama.cpp path with a stub curl serving a wrong file → `state=failed`, `reason` mentions the checksum, `friday.env` untouched; `--first-login` with `state=ready` does nothing.
- [ ] Run — fails; implement; run — passes.
- [ ] Commit `feat(cli): friday local-ai install with Ollama or llama.cpp`.

### Task 3: Installer AI step: several providers and local AI

**Files:** Modify `iso/friday/airootfs/root/configurator` (`step_ai`, summary, export list), `iso/friday/airootfs/root/installer` (keys loop, local choice, pacstrap engine package, enable unit); Test: `iso/friday/test/checks/installer_flow.py`, `installer.sh` stub env line.

**Consumes:** `friday local-ai detect|presets` (Task 1). **Produces:** env `FRIDAY_AI_PROVIDERS="gemini groq"`, `FRIDAY_AI_KEYS` (newline-separated `provider=key`, exported only to the installer), `FRIDAY_LOCAL_ENGINE=ollama|llamacpp|`, `FRIDAY_LOCAL_BUDGET_MB=N`, `FRIDAY_LOCAL_MODEL=<id>`, `FRIDAY_LOCAL_GPU=nvidia|amd|intel|none`.

- `gum choose --no-limit`: the existing providers plus "Local AI (on this computer)". Nothing picked → Zen free as today. Keyed providers prompt in turn; empty key → note "skipped <name>".
- Local: engine choice, then a line "This computer: 16 GB RAM, AMD GPU 8 GB, 8 threads", then presets via `gum choose` built from `friday local-ai presets` (unavailable ones listed as notes, not choices); a too-small machine shows the reason and continues without local.
- Installer: write each key var to `friday.env`; `FRIDAY_CONTEXT_PROVIDER` = first vision-capable keyed provider. If local: pacstrap the Ollama package per Global Constraints (llama.cpp + GPU: `vulkan-icd-loader` and `vulkan-radeon`/`vulkan-intel`); write `local-ai.conf` with `state=pending`; link `friday-local-ai.service` into the user's `~/.config/systemd/user/default.target.wants/`; chown.
- [ ] Flow test: pick Gemini + Groq + Local (Groq key empty), Ollama, Balanced; expect "skipped Groq", the machine line, "sees screenshots" or "text only" per the fake tree, spec `ai=[gemini]` `local=ollama`. Existing default run still yields `ai=[]`.
- [ ] Installer check: run the installer's AI section with fake `/mnt` and assert `friday.env` (0600) has both keys and `local-ai.conf` + the wants link exist.
- [ ] Run — fails; implement; run — passes.
- [ ] Commit `feat(iso): choose several AI providers and local AI at install`.

### Task 4: `friday setup-ai local`

**Files:** Modify `packaging/linux/friday` (`friday_setup_ai` gains `local`), help text, `_friday` completion; Test: `local_ai.sh`.

- Interactive (gum when present): detect line, engine, presets, then runs `friday local-ai install` in the foreground with progress. Ollama missing → `sudo pacman -S --needed <pkg per GPU>` first.
- [ ] Test with stubs: `friday setup-ai local --engine ollama --preset balanced` (non-interactive flags) writes conf and calls install.
- [ ] Commit `feat(cli): friday setup-ai local`.

### Task 5: Routing and the not-ready message

**Files:** `services/intelligence/src/providers.rs` (test), `packaging/linux/friday` (ask failure text); Test: providers.rs test, `cli.sh`.

- [ ] Rust test `local_text_model_answers_text_and_a_screenshot_goes_to_cloud`: fabric with `llamacpp_model("qwen3:1.7b", false)` and a keyed vision cloud model; `select_for` text → llamacpp first; vision → llamacpp absent.
- [ ] `cli.sh`: with `local-ai.conf state=installing` and a "Couldn't reach your AI provider" failure, the notification says "Local AI is still downloading (friday local-ai status)".
- [ ] Commit `feat(cli): say when local AI is still downloading`.

### Task 6: Live check in the VM

- [ ] Build the ISO; in the VM pick Local (Ollama, smallest preset for the VM's 4 GB), install, log in, wait for the notification, ask a question with the network cut; record in the PR.
