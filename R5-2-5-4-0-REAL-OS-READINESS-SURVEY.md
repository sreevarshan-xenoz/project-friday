# R5-2-5-4-0 — Real-OS Readiness Survey (Omarchy target)

**Status:** FROZEN survey for R5-2-5-4. No implementation. This is a qualification
sequence, not a feature slice. Everything below is either a live check to run on
the real Omarchy machine, or a verified fact about the frozen code that **gates**
whether a check can ever pass.

**Inputs:** frozen R5-2-5-3-1 (`9228470`), all of R5-2-5-x, `dev` clean, `omarchy/` pristine at `0b3f1b7e`.

---

## 1. Headline finding (read first)

The readiness survey surfaced **one open seam that must be closed before R5-2-5-4-2
("FRIDAY can see") can pass**. It is not a hardware gap — it is a code gap already
openly declared by the frozen code's own doc comments. This is exactly what a
pre-hardware survey is for: it stops us promising a live vision demo the current
code cannot yet deliver.

**What is ready (transport of the gated envelope):**
`grim` capture → ContextGate → EgressGate → the fabric seam, with the real base64
screen bytes riding inside an `SCREEN_CAPTURE ... data=<b64>` envelope. All of that
is structurally wired and gate-enforced, and can be qualified on real hardware.

**What is NOT ready (true vision):**
1. **No model is registered for the `VisionFree` capability tier.** `complete_tier`
   resolves a model via `resolve_tier(provider, tier)` (`providers.rs:1060-1064`);
   if none is registered it errors `"no model registered for {provider} tier vision-free"`
   (`providers.rs:1170-1172`). Grep confirms **zero** `ModelInfo` declares
   `tier: Some(CapabilityTier::VisionFree)`. A unit test already asserts this
   (`providers.rs:3101-3106`). So the seam **errors at model resolution** today,
   before any bytes leave.
2. **No multimodal image block is ever constructed.** The base64 PNG is carried as
   **plain text** inside the `SCREEN_CAPTURE` string. Every provider request body
   emits `"content": <string>` (`providers.rs:291-299`, `470-478`, `543-551`; Copilot
   passes `-p <prompt>` text, `providers.rs:722-723`). A repo-wide search for
   `image_url` / `data:image` / `"type":"image"` content parts returns **zero matches**.
3. The frozen code already labels this as future work:
   - `context_read.rs:88-91` — "Full image/vision delivery is qualified on the Omarchy target in R5-2-5-4."
   - `context_read.rs:226-233` — "Routes the gated Screen payload to a **vision** capability model… qualified on the Omarchy target."

**Consequence / scope decision for the user (do not assume):** R5-2-5-4-2 in its
literal form ("send the image to a remote vision provider and answer from the
screenshot") **cannot pass on existing code alone** — it needs a **small seam
completion**: (a) register a vision-capable model on a `VisionFree`-reachable tier,
and (b) construct a real image content block (decode the base64 → `image_url` /
`data:image/png;base64,...`) in the provider body. That is an implementation step,
not a qualification. Options:

- **Option A — extend R5-2-5-4:** treat the seam completion as part of R5-2-5-4
  (R5-2-5-4-2 becomes "implement + qualify true vision"). Fastest path to the
  "FRIDAY can see" moment.
- **Option B — keep R5-2-5-4 qualification-only:** R5-2-5-4 passes on real capture +
  transport (R5-2-5-4-1) and the gates provably reach the seam; true vision becomes
  a separate slice R5-2-5-4-2 with its own implementation + hardware qualify.
  Keep R5-2-5-4 honest: it proves **gated screen-context transport**, not vision.

Both are legitimate. The survey's job is only to surface the fork. (Recommendation
leans **B** per the user's own "no implementation during a survey" discipline:
let R5-2-5-4 qualify what exists, then implement vision cleanly as its own step.
But it is the user's call.)

---

## 2. Scope of R5-2-5-4

```text
R5-2-5-4-0  real-OS readiness survey (this doc, NO implementation)
R5-2-5-4-1  real capture qualification — grim proves a real Wayland PNG round-trips the gates
R5-2-5-4-2  real vision qualification — (blocked by §1 open seam unless Option A)
   ↓ green
R5-2-6     FRIDAY OS adaptation decision (NOT ISO yet) — boot identity, packages,
           defaults, FRIDAY shell, onboarding, installer, runtime packaging, update path
```

**"No implementation" everywhere in R5-2-5-4-0/1.** The moment any code must change
to make a check pass (the vision seam), we are no longer in qualification.

---

## 3. Inventory — verified from frozen code (provable now, off-hardware)

| Readiness item | Verified fact (file:line) | Needs live machine? |
|---|---|---|
| Real collector is wired | `friday-linux-daemon.rs:235` uses `RealCommandRunner::default()` | no |
| Gates connected in prod path | daemon → `ScreenReader` → `ContextGate` → `EgressGate` → `FabricProviderSeam` (`context_read.rs:118-213`, daemon `227-238`) | no |
| Consent enforced | `consent: true` → `context_reader.consent()`; without it `read()` blocked at gate, provider sees nothing (`friday-linux-daemon.rs:306-317`, `context_read.rs:317-356` tests) | no |
| Screen capture = `grim` | `grim -` fullscreen / `-g <region> -` (`collector.rs:193-216`) | **yes** (binary + Wayland session) |
| App = `hyprctl -j activewindow` | `collector.rs:241-250` | **yes** (only if App exercised) |
| Clipboard = `wl-paste --no-newline` | `collector.rs:254-266` | **yes** (only if Clipboard exercised) |
| Selection = `omarchy-capture-text` (grim+tesseract) | `collector.rs:218-239`; `TextSource::OcrRegion` | **yes** (only if Selection exercised) |
| Provider mapping | env `PROVIDER_ENV_VARS` (`providers.rs:23-34`): `OMNIROUTE/OPENROUTER/GROQ/GEMINI/SAMBA/GITHUB_TOKEN/SILICONFLOW/ZHIPU` + `copilot` CLI; free allowlist `providers.rs:2098-2111` | **yes** (keys / login) |
| Vision tier routing | **BROKEN as of today** — no `VisionFree` model registered (`providers.rs:1060-1064`, `1170-1172`, test `3101-3106`) | no (code fact) |
| Multimodal delivery | **ABSENT** — text-only request bodies, zero image content blocks | no (code fact) |
| daemon state path | `~/.local/state/friday/state.json` | no |
| `friday.context` panel | third-party plugin; manifest valid (`9228470`); **QML rendering NOT verifiable on Windows** | **yes** (rendering + hotkey) |
| `omarchy/` integrity | pristine at `0b3f1b7e`; not committed to; FRIDAY adaptation in `friday-os/plugins/` | no |

---

## 4. Hardware-gated checklist (run on the real Omarchy machine)

These are the checks the survey cannot complete from the Windows dev box. Each is
a pass/fail gate.

### 4.0 Omarchy substrate
- [ ] Boot Omarchy at upstream `0b3f1b7e` (branch `quattro`). Record `git -C omarchy rev-parse HEAD` == `0b3f1b7e`.
- [ ] `omarchy-shell` runs; default shell session is up.

### 4.1 FRIDAY runtime alongside Omarchy
- [ ] `friday-linux-daemon` builds on the target (`cargo build --release -p friday-mobile --bin friday-linux-daemon`) and starts.
- [ ] `friday-ctl` is on `$PATH` (`command -v friday-ctl`).
- [ ] `friday.context` is discovered by Quickshell: present in `~/.config/omarchy/plugins/friday.context`, enabled via `~/.config/omarchy/shell.json` `plugins:[{"id":"friday.context"}]`, `omarchy-shell shell toggle friday.context` summons it.
- [ ] Hotkey installed via FRIDAY provisioning (`~/.config/hypr/bindings.lua`), **not** an edit to `omarchy/`.
- [ ] `GRIM_SLURP`/tooling present: `command -v grim hyprctl wl-paste` (Screen path needs `grim`; others only if exercised).
- [ ] `FRIDAY_CONTEXT_PROVIDER` set to a registered, authenticated provider (e.g. `groq` with `GROQ_API_KEY`, or `copilot` after `copilot login`).

### 4.2 Integrity
- [ ] No writes ever land in `omarchy/` (GitHub-style protection: `git -C omarchy status --short` stays clean during/after FRIDAY operations).

---

## 5. Verification ordering

1. **4.x readiness** — all pass ⇒ proceed.
2. **R5-2-5-4-1 (capture envelope):** hotkey → Share → `friday-ctl context screen --yes`
   with `FRIDAY_CONTEXT_PROVIDER` set; expect a `CommandResult`; inspect that the
   provider seam received `SCREEN_CAPTURE mime=image/png … data=<b64>` with a
   nonzero `bytes=`. This proves real Wayland `grim` output traversed
   ContextGate → EgressGate. **Passes with current code.**
3. **R5-2-5-4-2 (true vision):** **blocked by §1** unless Option A (seam completion)
   is accepted. On Option A: `friday-ctl` baseline unauthenticated latency, then the
   "what application is currently open, and what is the main thing displayed?"
   question answered correctly from the actual pixels.

---

## 6. Honest labels

- Until R5-2-5-4-1 + readiness pass: **"screen-context transport (gated)"**.
- Only after R5-2-5-4-2 (Option A) passes: **"FRIDAY can see"** — and even then,
  vision is one *channel*, not general perception.

---

## 7. Next-step decision (return to user)

Choose the R5-2-5-4-2 scope fork (§1). Whether or not seam completion is included,
do **not** start any Omarchy → FRIDAY OS transformation (R5-2-6) before R5-2-5-4
goes green. The engine must pass its road test before we build the chassis.
