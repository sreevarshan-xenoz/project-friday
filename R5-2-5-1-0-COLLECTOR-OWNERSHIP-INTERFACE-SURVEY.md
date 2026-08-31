# R5-2-5-1-0 — Context Collector Ownership + Interface Survey (FROZEN, NO CODE)

**Status:** FROZEN survey step. **Zero implementation.** Decides *who owns the OS capture calls* and *what the collector interface contract is*, before the minimal collector slice (R5-2-5-1).
**Inputs:** frozen `R5-2-5-0-OS-CONTEXT-INVOCATION-SURVEY.md` (corrected: collector ownership deliberately left to this step), `R5-1-0-INTEGRATION-CONTRACT.md`, `R5-2-4` Context Gate contract (`services/intelligence/src/context_gate.rs`), `omarchy/@0b3f1b7`.
**Starting point:** `dev @ fcb1676`, working tree clean.

---

## 1. The question

Where does the data flow *physically* run? Three candidate owners for the shape of the collector:

```text
ContextChannel
   │
   ▼
Context Collector  →  ContextGate  →  EgressGate  →  Provider
(WHO owns capture?)
```

The collector is a **pure OS capability** — its job is only:

> "Here is the context the OS can provide."

It **must not decide whether context is allowed to leave the machine.** That is `ContextGate`'s job. Otherwise the screenshot collector quietly becomes a privacy-policy engine wearing a fake moustache. The interface contract must make that separation structural, not editorial.

---

## 2. Candidate owners (surveyed against the frozen substrate)

### A. The `friday.shell` plugin (Quickshell QML)
- **Substrate reality:** the plugin already triggers capture UX (`Panel.qml:96` `root.bar.run`) and region pickers; Omarchy commands are shell-called today.
- **Why it fails as *primary* owner:** it is a **UI layer**. Embedding the collector there would couple capture to a running shell session, duplicate the query if the shell restarts, and bury an OS capability inside QML. R5-1-0 keeps the shell as *UI + first-capture UX*, not orchestration.
- **Right role:** the *context chooser* UI (R5-2-5-3), and possibly *first-capture UX* (region picker for Selection). Not the collector.

### B. The `friday-linux-daemon`
- **Substrate reality:** the daemon owns `GoalRunner`, `Executive`, the provider path, WS ingress, `state.json` (R5-1-0). It is the natural **orchestrator** of collect→gate→egress→provider.
- **Why it fails as *primary* owner of capture:** the daemon is an orchestration/runtime service. Giving it `grim`/`hyprctl`/`wl-copy`/`tesseract` subprocess calls directly couples a lifecycle-managed orchestrator to OS-specific command quirks, and risks the collector logic *growing* into the daemon over time.
- **Right role:** **orchestrator** — the daemon *calls* the collector, then `ContextGate::authorize`, then `EgressGate`, then the provider. It owns the flow, not the capture.

### C. A dedicated OS-capture adapter (collector in `services/`)
- **Substrate reality:** a thin Rust module/crate wrapping Omarchy's capture commands behind the `ContextChannel → captured-context` seam. Spawning subprocesses is **already the established FRIDAY pattern** — `CopilotCliProvider` shells to the `copilot` CLI via `std::process::Command` (`providers.rs:719, 827, 4007`). In-process (no new IPC), per R5-1-0.
- **Why it wins:** it is a **pure capability** — the exact "here is what the OS can provide" contract. It makes **no privacy decision**. It is testable against command mocks/CLI contracts (proven strategy in `services/windows-qual`, `services/provider-qual`). It keeps the daemon lean (orchestrator) and the shell thin (UI/chooser).

### Recommendation
> **The collector is an OS-capture adapter in `services/` — a library behind `ContextChannel → captured-context`, spawned as a thin runner for CLI/integration use. The daemon orchestrates (calls collector → gate → egress → provider); the shell stays the UI/chooser.** No new IPC framework; in-process; subprocess-to-`omarchy-*`/`grim`/`hyprctl` calls, matching the Copilot-cli precedent.

```text
friday.shell (UI/chooser, R5-2-5-3)
        │
        ▼
friday daemon (orchestrator)
        │  calls
        ▼
Collector (OS capability in services/)   ← owns grim / hyprctl / wl-copy / OCR
        │   captured payloads (NO privacy decision)
        ▼
ContextGate::authorize (consent / revoke / disclosure)
        ▼
EgressGate (secret safety)
        ▼
Provider → FRIDAY response
```

---

## 3. The collector interface contract (frozen shape)

The collector exposes **one domain concept**: produce the OS context the OS can provide, **per channel**, without deciding anything about whether it may leave.

```text
ContextChannel::{ Screen, Selection, App, Files, Mic, Clipboard }
        │
        ▼
collector.capture(channel, capture_options) → CapturedContext
```

**Frozen contract:**

1. `ContextChannel` is the existing enum from R5-2-4 (`services/intelligence/src/context_gate.rs`) — the collector does **not** define its own channel vocabulary.
2. `capture(channel, …)` returns a channel-typed **`CapturedContext`** — the raw OS-provided payload only:
   - `Screen` → `CapturedContext::Image { bytes: Vec<u8>, mime: image/png, geometry: Option<Rect> }` (from `grim` via `omarchy-capture-screenshot`).
   - `Selection` → `CapturedContext::Text { text: String, source: OcrRegion }` (from `omarchy-capture-text`: grim region → OCR).
   - `App` → `CapturedContext::AppInfo { class, title, geometry, focused: bool }` (from `hyprctl clients`).
   - `Clipboard` → `CapturedContext::Text { text: String, source: Clipboard }` (from `wl-copy`-history / clipboard).
   - `Files` and `Mic` → **OUT of V1** (R5-2-5-1-0 defers them: their capture semantics differ and belong in later slices). The contract still type-checks them but the collector returns an explicit `Unavailable { channel, reason }` until implemented.
3. **No privacy decision in the collector.** The return has **no** `allow`/`consent`/`providerTier` semantics. `ContextGate::authorize` is the only place a channel is allowed or blocked.
4. **Capture is bounded and explicit.** The collector captures *only* the channels a caller asks for, never ambiently. No "collect everything" API surface (per R5-2-5-0: no ambient surveillance).
5. **Capture options are channel-scoped** (e.g. `Selection` takes an optional region rect from the shell's picker; `Screen` takes fullscreen|focused-monitor) — the chooser may pass them; the collector never invents scope.
6. **Errors are capture errors only** (`CaptureError::Command { channel, message }`, `CaptureError::Unavailable { channel, reason }`, `CaptureError::Timeout`) — **never** privacy verdicts.
7. **Testability:** the collector is testable against a fake command runner injecting canned stdout (the same contract-test strategy as the repo's existing qual layers) so a Windows dev host proves ordering without real Wayland.

**Composition with R5-2-4 (unchanged):** the collector output feeds `ContextGate::authorize(channel, target)` then `EgressGate::process` exactly as R5-2-4 freezes. The collector merely produces inputs; it is upstream of, and separate from, both gates.

---

## 4. What this survey does NOT lock

- ❌ The **physical crate** / module path for the collector (decided in R5-2-5-1 implementation; the survey fixes *ownership* and *interface*, not layout).
- ❌ Whether the daemon's goal protocol (WS) carries a context-chooser payload natively vs the chooser calling the collector-then-gate then provider (decided in R5-2-5-2).
- ❌ `Files` / `Mic` capture semantics (deferred).
- ❌ Any QML / `friday.shell` changes (R5-2-5-3).

---

## 5. Acceptance boundary for R5-2-5-1-0 (spec freeze)

- [x] Ownership surveyed against the frozen substrate (shell vs daemon vs dedicated adapter) with rationale.
- [x] Recommendation frozen: **collector = OS-capture adapter in `services/` (in-process library + thin runner); daemon orchestrates; shell stays UI/chooser.**
- [x] Interface contract frozen: `ContextChannel` reused from R5-2-4; `capture(channel, options) → CapturedContext`; **zero privacy semantics in the collector**; channel-scoped explicit capture; capture errors only.
- [x] `Screen / Selection / App / Clipboard` in V1; `Files / Mic` deferred with a defined `Unavailable` shape.
- [x] Zero implementation — no `services/` source, no QML, no protocol changes.

**Next:** R5-2-5-1 — minimal collector implementation (branched from `dev @ fcb1676`), unit-tested against a fake command runner, `Screen / Selection / App / Clipboard` in V1, `Files / Mic` returned as `Unavailable`.