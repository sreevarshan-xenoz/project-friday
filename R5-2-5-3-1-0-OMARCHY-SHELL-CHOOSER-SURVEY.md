# R5-2-5-3-1-0 — Omarchy FRIDAY Shell Chooser: Reuse + Ownership Survey

**Status:** SURVEY (frozen, no QML, no `friday-ctl` change yet).
**Question this survey answers:** how do we put the proven `friday-ctl context screen` consent flow into the Omarchy desktop **without duplicating** the security logic, by reusing the existing Omarchy substrate?
**Boundary inputs:** frozen R5-2-5-3 (`71d0986`), frozen R5-2-5-3-0 (`3a49a98`), frozen R5-2-5-2 (`26a0e41`), `dev @ 71d0986`.

---

## Design principle (from the user's direction)

The QML panel **must never become a second ContextGate**. It expresses a single intent — *"I want to share Screen"* — and nothing more. Consent, disclosure, channel validation, egress safety, and provider transmission all remain authoritative in the Rust path (`ContextGate → EgressGate → provider`, proven through R5-2-5-2). The UI only supplies the user's explicit yes/no; the security gate does the enforcing. This keeps the UI replaceable and makes it impossible to bypass the security boundary by editing QML.

---

## What the Omarchy substrate already offers (authoritative survey)

### 1. Panel plugin contract (kind `"panel"`)
- Manifest `manifest.json` at the plugin root: `{ schemaVersion, id, name, version, kinds: ["panel"], keepLoaded, entryPoints: { panel: "Panel.qml" } }` — mirror `omarchy.speedtest` (`shell/plugins/panels/speedtest/manifest.json`). Registered by `shell/services/PluginRegistry.qml`.
- `Panel.qml` must expose `open(payloadJson)` / `close()` lifecycle, summoned via `shell.summon(id, payload)` / `shell.toggle(id)` / `shell.hide(id)`. First-party plugins are **implicitly enabled** — no `shell.json` edit needed.
- **No `shell/shell.qml` edit needed** for a stock panel: the generic `shell summon/toggle/hide` IPC target already reaches any manifest-declared plugin id. A custom `IpcHandler` alias (like `image-selector`) is optional, not required.

### 2. Hotkey → panel
- Hyprland Lua bindings (`config/hypr/bindings.lua` in user space; `default/hypr/bindings/*.lua` as defaults). Model:
  ```lua
  o.bind("SUPER + CTRL + F", "FRIDAY context", "omarchy-shell shell toggle omarchy.friday-context")
  ```
- `bin/omarchy-shell` is the canonical IPC entry point; it never starts the shell.

### 3. CLI execution + file read from QML
- Fire-and-forget: `Quickshell.execDetached([...argv])` (shell/Commons/Util.qml).
- Capture output: `Process {}` from `Quickshell.Io` with `StdioCollector`/`SplitParser` (image-picker, speedtest).
- Read files: `FileView {}` from `Quickshell.Io` with `onLoaded: text()` (clipboard history, shell config).

---

## The one non-obvious design fork: consent lives in the UI, not in the CLI's TTY

The R5-2-5-3 CLI flow is **interactive**: `confirm_context_consent()` reads `y/N` from stdin. That works for a terminal, but a Quickshell panel has **no TTY** — if the panel shells out to `friday-ctl context screen` as-is, the interactive prompt is unanswerable and the flow dead-ends.

Resolution that preserves "QML is not a second ContextGate":
- **Consent is UI by nature** — the Share/Cancel buttons in the panel *are* the consent UX, exactly as the `[y/N]` prompt is in the terminal. Consent is never a Rust-side boolean decision; it is the user's expressed intent.
- The panel shows the disclosure and the Share/Cancel choice, then on **Share** invokes the client path with `consent: true` — the same per-use flag the frozen R5-2-5-2 seam requires. The Rust gate still validates and enforces; the panel only conveys the user said "yes".

### Where the disclosure text comes from (no QML duplication)
The canonical Screen disclosure currently comes from `ContextGate::disclosure_for(...)` via `friday-ctl`'s `screen_disclosure()`. To keep that wording authoritative and not hard-code it into QML, `friday-ctl` should expose a **non-interactive `context describe`** subcommand that prints the disclosure (and the provider/channel) as JSON for the panel to render via `Process`/`StdioCollector`. The security text stays in one Rust source; the panel is a pure renderer.

### Non-interactive send path
The panel needs to send `context.read { consent: true }` **without** the interactive prompt (consent already collected by the buttons). Two options:

- **Option B1 (recommended):** extend `friday-ctl` with a `context screen --assume-consent`-style non-interactive flag (or a separate `context screen-yes`), gated behind an explicit caller-supplied flag. The panel runs `Quickshell.execDetached(["friday-ctl", "context", "screen", "--yes"])` after the user taps Share. Keeps **all** client/WebSocket/token/error plumbing in one binary; the panel stays a dumb UI. The security logic is unchanged (`consent: true` must still satisfy the frozen gate).
- **Option B2:** the panel talks WebSocket to the daemon directly, duplicating `friday-ctl`'s client plumbing in QML. Rejected: that *is* a second implementation of the transport, exactly what we avoid.

**Recommendation: Option B1.** The command is a one-line CLI string the panel executes; consent + disclosure + connection + error surfacing all live in `friday-ctl`, reusing R5-2-5-3's proven code. No client logic leaks into QML.

---

## Minimal file set to implement (R5-2-5-3-1, pending this survey)

**Add:**
1. `omarchy/shell/plugins/friday-context/manifest.json` — `kind: ["panel"]`, `entryPoints.panel: "Panel.qml"`.
2. `omarchy/shell/plugins/friday-context/Panel.qml` — thin UI: title + disclosure (from `Process` → `friday-ctl context describe`) + **Share / Cancel** buttons. `open()`/`close()` lifecycle. On Share → `Quickshell.execDetached(["friday-ctl", "context", "screen", "--yes"])`; on Cancel → `close()`.

**Edit:**
3. `omarchy/config/hypr/bindings.lua` — hotkey binding to `omarchy-shell shell toggle omarchy.friday-context` (SUPER + CTRL + F).

**Edit (Rust, minimal, `friday-ctl.rs` only — the reuse seam):**
4. Add `friday-ctl context describe` (non-interactive disclosure JSON) and a non-interactive consent-skipped `context screen --yes` path. `cargo fmt` scoped strictly to this file.

**What does NOT change:**
- `shell/shell.qml`, plugin registry, shell.json config, other plugins.
- The daemon, ContextGate, EgressGate, collector, providers, protocol — all frozen and untouched.
- No new IPC target (stock `shell toggle` suffices); no ambient/automatic capture.

---

## Frozen boundary / scope guard (asserted)

In scope for **R5-2-5-3-1** (after this survey is approved):
- One panel plugin (manifest + QML), one hotkey binding, and the small non-interactive `friday-ctl` extension (Option B1) as the only Rust touch.
- The panel renders disclosure from `context describe` and expresses Share/Cancel; it never re-validates channels, never decides egress, never contacts the provider directly.

Out of scope (deferred):
- ❌ Real capture / vision provider qualification (R5-2-5-4).
- ❌ Selection / App / Clipboard channel variants.
- ❌ ChatGPT-web / CDP, ambient/automatic context, new IPC architecture.
- ❌ Any change to the frozen gate/providers/collector/daemon.

**Frozen file:** this survey (`R5-2-5-3-1-0-OMARCHY-SHELL-CHOOSER-SURVEY.md`). No QML and no `friday-ctl` edits until this is approved.

---

## Suggested next slice (pending approval)
`R5-2-5-3-1` — implement the panel plugin + hotkey + the small non-interactive `friday-ctl` seam, proving "hotkey → panel → Share → `context.read → ContextGate → EgressGate → provider`" with consent expressed in the UI and enforced by the Rust gate.
