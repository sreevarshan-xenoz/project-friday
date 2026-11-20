# Local AI and several providers at install

## Why

Today the installer offers one cloud provider and no local option. Zen's free
tier now refuses other apps, so a person who wants FRIDAY to keep working (and
to keep things private) needs more than one way in: several cloud providers,
and a model on their own machine sized to what that machine can do. The
installer should tell them plainly what they get, including "text only" when
the machine is small.

## Decided by the owner

- **Several providers:** the AI step is a multi-select; each keyed provider
  asks for its key. Local AI is one of the choices.
- **Local first:** with local and cloud both chosen, the local model answers;
  cloud handles what local can't (a screenshot on a text-only model) or when
  local fails or isn't ready.
- **Budget as presets with GB:** worked out from this machine; each preset
  shows the model it gets and whether it can see screenshots.
- **Two engines:** Ollama (Arch package) and llama.cpp (its official GitHub
  release build, pinned by version and SHA-256, unpacked under the user's
  home; an owner-approved exception to "packages come from the Arch repos").
- **Download at first login, in the background:** the install stays fast;
  cloud answers until the model is ready, then a notification says local AI is
  on. With no cloud chosen, FRIDAY says the model is still downloading.

## Approaches considered

1. **One `friday local-ai` command shared by the installer and the installed
   system (chosen).** `friday-git` is on the live ISO, so the installer calls
   `friday local-ai detect|presets` to show real numbers, saves the choice, and
   the installed system runs `friday local-ai install` at first login. One
   copy of the logic; `friday setup-ai local` and `friday private on` use it too.
2. Detection and the model table inside the ISO configurator. Rejected:
   FRIDAY's logic would live in `airootfs/` (against the packaging rule) and
   drift from `friday private on`.
3. A Rust hardware probe in the daemon. Rejected for now: the installer and
   CLI are bash, and nothing here needs the daemon.

## Design

### Hardware detection: `friday local-ai detect`

`friday local-ai summary` gives the machine in one line plus caveats (NVIDIA
memory unknown, old CPU). `detect` prints `key=value` lines (tests point `FRIDAY_HW_ROOT` at a fake `/proc` and
`/sys`):

- `ram_mb`: MemTotal.
- `gpu`: `nvidia|amd|intel|none` from PCI display devices
  (`/sys/bus/pci/devices/*/class` 0x03xx), so a GPU with no driver loaded
  still counts; Intel and AMD APUs (under 2 GB VRAM) count as integrated.
- `vram_mb`: AMD from `mem_info_vram_total`; NVIDIA from `nvidia-smi` when the
  driver is loaded, else `unknown`. Unknown VRAM plans on RAM; the summary
  says `friday setup-ai local` offers a GPU preset once the driver is in.
- `cpu_threads`, `avx2` (`yes|no` from `/proc/cpuinfo`).
- `disk_free_mb` for the home filesystem (installer: the target disk size).

### Budget presets: `friday local-ai presets`

Memory local AI may use, from RAM (and VRAM on a discrete GPU, where models
run fast):

| Preset | Budget |
|---|---|
| Light | 25% of RAM |
| Balanced | 50% of RAM |
| Most | 75% of RAM |
| GPU (discrete GPU with known VRAM) | VRAM minus 0.5 GB |

Each preset maps to the largest model that fits (model size + ~1 GB for
context and runtime):

| Fits in | Model (Ollama tag / GGUF) | Download | Screenshots |
|---|---|---|---|
| under 2 GB | none: "too little memory for local AI; use a cloud provider" | | |
| 2 GB | qwen3:1.7b | 1.4 GB | no: text only |
| 3.5 GB | qwen3:4b | 2.5 GB | no: text only |
| 4.5 GB | gemma3:4b | 3.4 GB | yes |
| 9 GB | gemma3:12b | 8.2 GB | yes |
| 19 GB | gemma3:27b | 17.4 GB | yes |

A preset whose model won't fit on disk is shown as unavailable. No AVX2 and no
GPU: shown, with "this computer is old for local AI; answers will be very
slow". Shown per preset: model, download size, text-only or sees screenshots,
"fast (GPU)" or "slower (CPU)".

### Installer AI step

1. Multi-select: cloud providers (as today) plus "Local AI (on this computer)".
   Nothing picked = Zen's free models, as today.
2. A key prompt for each keyed provider picked.
3. If local was picked: engine (Ollama / llama.cpp), then the presets from
   `friday local-ai presets`, with the detected machine on one line above
   ("16 GB RAM, AMD GPU 8 GB, 8 threads"). The text-only and too-small cases
   are said in words, not hidden.
4. The summary screen lists every provider and the local model.

The installer writes the keys to `friday.env` as `friday setup-ai` does, and
the local choice to `~/.config/friday/local-ai.conf` (`engine=`, `budget_mb=`,
`model=`).

### First login: `friday local-ai install`

Run once by a user unit (`friday-local-ai.service`, oneshot, enabled by the
installer when local was picked):

- Ollama: `pacman` package by GPU (`ollama-cuda` NVIDIA with driver,
  `ollama-rocm` AMD, else `ollama-vulkan` with a GPU, `ollama` CPU), then
  `ollama pull`. The package install needs root, so the installer pacstraps
  the Ollama package; first login only pulls.
- llama.cpp: download the pinned `ubuntu-vulkan-x64` (GPU) or `ubuntu-x64`
  (CPU) tarball and the model's GGUF (+ `mmproj` for vision) from pinned
  Hugging Face revisions, check SHA-256, unpack to
  `~/.local/share/friday/llama.cpp`, and run `llama-server` as a user unit
  (`friday-llama.service`) on 127.0.0.1:8080.
- Writes `LLAMACPP_ENABLED/BASE_URL/MODEL/VISION` to `friday.env`, restarts the
  daemon, and notifies "Local AI is on: <model> (text only|sees screenshots)".
  Progress shows in `friday local-ai status`; failure notifies with the reason
  and leaves cloud answering.

### Routing

Already in place: the local model ranks first (120 ms), vision requests skip a
text-only model, and an unreachable local server falls back to cloud. Added: a
test of "local and cloud both set: local answers text, cloud answers a
screenshot on a text-only local model". With only local chosen and the model
not ready, the answer says it's still downloading, not "can't reach your
provider".

### After install

`friday setup-ai local` runs the same detect → engine → presets → install
interactively. `friday private on` uses the same presets in place of its
RAM-only guess.

## Non-goals

- Windows (the MSI has its own setup; later).
- Picking models from the whole catalogue: the ladder above, kept current by
  the weekly model check.
- Running local models on the live ISO.

## Tests

- `friday local-ai detect|presets` against fake `/proc` and `/sys` trees:
  2 GB, 8 GB, 16 GB + AMD 8 GB, NVIDIA unknown VRAM, no AVX2, small disk.
- `installer_flow.py`: multi-select, two keys, local + presets screen, the
  text-only and too-small wording, the summary.
- The routing test above (Rust).
- The ISO install test with local picked (Ollama, smallest model) in the VM.

## Left to the builder

- Exact GGUF repos, revisions and checksums, and the llama.cpp build pin.
- Wording of each screen, in the installer's existing voice.
