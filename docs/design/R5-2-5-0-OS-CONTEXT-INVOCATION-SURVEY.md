# R5-2-5-0 — OS Context & Invocation Survey (FROZEN, NO CODE)

**Status:** FROZEN survey step for **R5-2-5: Contextual OS Assistant**. **Zero implementation.** This is a read-only architecture map that answers the five questions before any QML, Wayland capture, or provider-path work begins.
**Inputs:** frozen `R5-1-0-INTEGRATION-CONTRACT.md` (Omarchy/Quickshell shell contract), frozen `R5-2-4-CONTEXT-GATE-SPEC.md` + `-IMPLEMENTATION.md` (the consent contract this survey's experience layer will drive), frozen `R5-0-OMARCHY-AUDIT.md` (substrate map), `omarchy/` at `0b3f1b7`.
**Starting point:** `dev @ 19ec6b8`, working tree clean.

---

## 0. Framing — what R5-2-5 actually is

R5-2-5 is not "OS-level reasoning" and definitely not "the chat box." It is:

> **FRIDAY can understand the user's current computer context — but only when the user deliberately gives it that context.**

The hard engineering is turning the frozen Omarchy substrate + FRIDAY runtime into an actual **FRIDAY OS**: the Context Gate becomes *visible and useful* to the person in front of the machine. R5-2-5 builds the **experience** on the R5-2-4 **contract**. It does **not** rebuild the provider path and does **not** build a banned ChatGPT-web proxy.

**V1 (this phase, brutal smallness):**

> **Press FRIDAY hotkey → choose context → FRIDAY answers.**

Not: *FRIDAY continuously watches everything on the computer*. No ambient screen surveillance — privacy nightmare and technically unnecessary. (The `ContextGate` design already punishes this: it's per-use consent; ambient capture would need consent on every frame.)

---

## 1. How does FRIDAY know what's on screen?

Answer: **Omarchy already ships the entire capture surface** on the frozen substrate. Nothing needs inventing. We map each `ContextChannel` to an existing Omarchy command/mechanism:

```
ContextChannel   →  Omarchy mechanism (frozen substrate, grim/slurp/hyprpicker/hyprctl/wl-copy/tesseract)
────────────────────────────────────────────────────────────────────────────────────────────────
Screen           →  omarchy-capture-screenshot  (grim + slurp: region|window|fullscreen|smart → PNG file/clipboard)
                    omarchy-capture-region      (shared picker, slurp + hyprpicker screen-freeze)
Selection        →  omarchy-capture-text        (grim region → tesseract OCR → text → wl-copy)
                    [no at-spi focused-app text-selection seam exists; OCR-of-region is the honest path today]
App              →  omarchy-hyprland-focus-app   (hyprctl clients → .class/.initialTitle)
                    omarchy-hyprland-monitor-*   (hyprctl monitors: focused monitor, geometry, scaling)
Files            →  FRIDAY's existing file capabilities / goal path (no new OS capture)
Mic              →  FRIDAY VoiceIntake / wireplumber (existing; R5-2-5 does not add mic capture)
Clipboard        →  omarchy-clipboard-paste-text (wl-copy/wtype) + shell plugins/clipboard (history at
                    ~/.local/state/omarchy/clipboard-history.json) + menu-clipboard
```

Key finding: **the capture substrate is green — the pieces already exist on `omarchy/@0b3f1b7` and are tested in production use.** R5-2-5's job is *composition + consent*, not capture invention.

---

## 2. How does the user invoke FRIDAY?

Frozen invocation path (all already exists):

```text
Hotkey (Hyprland keybind) / shell bar button / terminal
        │
        ▼
friday.shell plugin  ── IpcHandler { target: "friday.shell" }  (Panel.qml:135: goal(text))
        │            ── root.bar.run("friday-ctl goal …")      (Panel.qml:96, Omarchy launch seam)
        ▼
friday-ctl goal <text>   (services/mobile/src/bin/friday-ctl.rs)
        │   loopback WebSocket 127.0.0.1:18432, per-session token from state.json
        ▼
friday-linux-daemon (services/mobile/src/bin/friday-linux-daemon.rs)
        │   CommandGateway → plan.goal → GoalRunner → Executive
        ▼
   Context Request  →  Context Gate  →  approved collector  →  Egress Gate  →  provider
```

Per R5-1-0, this **reuses** Omarchy's existing hotkey/launch mechanisms — no new hotkey daemon, no second IPC framework, no second shell architecture. The hotkey bindings live in Omarchy's Hyprland keybinding config (the same layer that binds Super+Alt+, screenshot editing and the slurp selection modes).

---

## 3. Where does the Context Gate actually receive its request?

The current `ContextGate` (`services/intelligence/src/context_gate.rs`) is a **pure contract** — `ContextChannel` / `ContextGate::authorize(channel, ProviderTarget)` / `ContextDecision` / `compose(gate, egress, channel, provider, target, payload)`. It has **zero OS capture** and **zero wiring** into the daemon. That is the exact gap R5-2-5 fills.

The **logical** data flow (independent of who owns the capture calls):

```text
OS capture mechanism (grim / hyprctl / wl-copy / tesseract, already on substrate)
        │   produces OS context ("here is what the OS can provide")
        ▼
FRIDAY Context Collector   ← pure capability, NO privacy decision
        │   captured channel payloads
        ▼
ContextGate::authorize (per channel, per use)   ← consent / revoke / disclosure
        │   satisfied channels only
        ▼
EgressGate::process (secret safety)
        ▼
Reasoning Provider (Copilot / Groq / …) → FRIDAY response
```

The collector runs **only for channels the user consented to in the context chooser**, and each channel is fed to `ContextGate::authorize` as a `ProviderTarget`-tagged request before anything is assembled for the provider. An unauthorized channel never reaches the egress gate — exactly the R5-2-4 invariant.

> ⚠️ **Ownership deliberately NOT locked here.** This survey sketched the daemon command path as an early ingest candidate, but that over-commits. The collector is an **OS capability**; the daemon is an orchestration/runtime component. Whether the daemon, the `friday.shell` plugin, or a **dedicated OS adapter** owns the actual capture calls is decided in **R5-2-5-1-0** (collector ownership + interface survey), not here. We design the collector contract first, then place it — so we never end up with `QML → shell command → daemon → shell command → grim`.

---

## 4. How does FRIDAY get the context? (mechanism assessment vs frozen substrate)

Investigated against the frozen `omarchy/@0b3f1b7`:

| Mechanism | Status on substrate | Used by |
|---|---|---|
| **Hyprland IPC** (`hyprctl`) | ✅ present, heavily used | active app/window (`.class`, `.initialTitle`), focused monitor, geometry, workspace — `omarchy-hyprland-focus-app`, `omarchy-hyprland-monitor-*`, `omarchy-capture-region` |
| **Wayland screenshot** (`grim`) | ✅ present | `omarchy-capture-screenshot`, `omarchy-capture-text` |
| **Wayland screen-freeze picker** (`hyprpicker` + `slurp`) | ✅ present | `omarchy-capture-region` / `-screenshot` (freeze → select region/window/fullscreen) |
| **Clipboard** (`wl-copy`/`wtype`, `wlr-clipboard` history) | ✅ present | `omarchy-clipboard-paste-text`, shell `plugins/clipboard`, history JSON |
| **OCR** (`tesseract`) | ✅ present | `omarchy-capture-text` (selected text from a region) |
| **xdg-desktop-portal screen share** | ❌ not the path | avoided — Omarchy shell already does capture directly via grim/slurp; no portal needed for V1 |
| **at-spi / Wayland text-selection of focused app** | ⚠️ no seam found | OUT of V1 — selected text is OCR-of-region or clipboard, not programmatic focused-app text extraction |
| **Active-window metadata** | ✅ `hyprctl clients` | app/class/title + geometry for the `App` channel |
| **Accessibility APIs** | ⚠️ not wired on substrate | OUT of V1 |

**Decisions this produces (for the implementation slice):**
- Screen, App, Selection(OCR), Clipboard all map to existing Omarchy commands — **call them, don't reimplement them** (respects R5-1-0 "no second architecture").
- V1 uses the default **local/free path** where available to avoid remote data egress unless the user consents to a remote provider; the `ContextGate`+`compose` enforces the consent/disclosure regardless of provider class.

---

## 5. What should the first OS-level experience be? (V1 scope)

**V1 = "Press FRIDAY hotkey → choose context → FRIDAY answers."**

```
"Friday, look at my screen and explain this"
             │
             ▼ Invocation (hotkey / bar button)
   Context request  →  context chooser (shell popup)
             │          ☑ Screen      ☑ App       ☐ Selection   ☐ Clipboard
             ▼
      ContextGate::authorize per checked channel   (each per-use, target-tagged)
             │  Allow only for consented channels
             ▼
      approved context collector (Omarchy cmds above)
             ▼
      EgressGate::process (secret policy)
             ▼
      Copilot / Groq / … → FRIDAY Shell answer
```

**Explicit non-goals for R5-2-5 V1 (frozen):**
- ❌ No browser/CDP automation, no ChatGPT-web proxy (remains a separate future tool layer — a *tool*, not the reasoning path).
- ❌ No ambient / continuous screen monitoring.
- ❌ No new IPC framework, no new event bus, no second shell architecture (R5-1-0).
- ❌ No at-spi / focused-app text extraction; no xdg-desktop-portal dependency.
- ❌ No OS-level "reasoning" rewrite of the provider stack — R5-2-5 drives the existing fabric.
- ❌ No Omarchy config/state layout changes beyond adding FRIDAY's own entry seams (reuse `~/.local/state/omarchy` mirrors + NotificationServer).

**Post-V1 (future, explicitly not this phase):** legitimate computer-use / browser-control capabilities as a **separate tool layer** (the model uses them as tools; the reasoning provider is not itself a browser).

---

## 6. R5-2-5 coarse roadmap (survey output — for later freezing)

- **R5-2-5-0** this survey (frozen, no code).
- **R5-2-5-1-0** collector **ownership + interface survey** — decides whether the daemon, the `friday.shell` plugin, or a dedicated OS adapter owns the capture calls, and locks the collector interface contract (channels, no privacy decision, provider-agnostic). No code.
- **R5-2-5-1** minimal collector implementation — a small crate/service that wraps the chosen Omarchy capture commands behind the frozen `ContextChannel → captured-context` seam, **unit-tested against mocks/CLI contracts** (no real Wayland needed to prove ordering).
- **R5-2-5-2** integration — thread the collector + `ContextGate::authorize` (+ `compose` with `EgressGate`) into the path chosen by 5-1-0; extend the `friday-ctl`/shell `goal` protocol to carry a context-chooser payload only if the chosen owner requires it.
- **R5-2-5-3** shell context chooser — the `friday.shell` popup (hotkey → check channels → submit), reusing Omarchy Quickshell + NotificationServer; the moment the R5-2-4 gate becomes visible.
- **R5-2-5-4** qualification + freeze (test the V1 vertical slice end-to-end, mocks for Wayland where the dev host is Windows — same strategy the repo already uses in `services/windows-qual`, `services/provider-qual`).

Each of 5-1-0…5-4 is its own **prove → implement → qualify → freeze** slice, branched from `dev`, no cross-cutting refactor.

---

## 7. Acceptance boundary for R5-2-5-0 (spec freeze)

- [x] Five questions answered from verified primary sources (`file:line` in report body).
- [x] Capture surface found to already exist on frozen substrate (grim/slurp/hyprpicker/hyprctl/wl-copy/tesseract + Omarchy wrapper commands).
- [x] Invocation path mapped (hotkey/bar → `friday.shell` `goal(text)` → `friday-ctl` → daemon `plan.goal`).
- [x] Context Gate logical data flow identified (collector + `authorize` + `compose`), preserving per-use consent and independent EgressGate; **collector ownership deliberately left to R5-2-5-1-0, not locked here**.
- [x] V1 scope frozen to "hotkey → choose context → FRIDAY answers" — no ambient surveillance, no browser proxy.
- [x] Zero implementation — no `services/` source, no QML, no Wayland/protocol changes.

**Next:** R5-2-5-1-0 collector ownership + interface survey (no code), then 5-1 (minimal collector), then 5-2 (integration at the 5-1-0-chosen seam), then 5-3 (shell chooser), then 5-4 (qualify + freeze).
