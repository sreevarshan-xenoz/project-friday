# R5-2-5-3-0 — Shell Context Chooser: Ownership + State Survey

**Status:** SURVEY (frozen, no code). Not yet implemented.
**Question this survey answers:** where should the shell chooser live, and what is the smallest existing Omarchy / `friday.shell` interaction we can reuse — without creating another UI or IPC architecture?
**Boundary inputs:** frozen R5-2-5-2 (`26a0e41`), frozen R5-2-5-1 (`e3cdba7`), frozen R5-2-4 gate (`19ec6b8`), `dev @ 26a0e41`.

---

## Pivotal finding: there is NO existing `friday.shell` interaction in Omarchy

The daemon documentation from R5-1/R5-2 describes a *planned* flow: "boot → daemon starts → shell discovers `friday.shell` → shell renders live state from `state.json`". This reads as if a `friday.shell` panel/extension already exists.

**It does not.** An exhaustive search of the Omarchy tree found **zero** real FRIDAY integration — no plugin manifest, no QML panel, no `IpcHandler`, no config, no command. The only `friday` string matches are the weekday name in clock/weather models and a `'Friday 14:42'` timestamp in the clipboard test — all false positives.

The Omarchy shell knows nothing about FRIDAY. There is no `friday.shell` IPC target, no FRIDAY panel, no `friday-ctl` command surface beyond `goal` and `system-info`. **R5-2-5-3 therefore cannot "reuse an existing friday.shell chooser" — it must create the first FRIDAY touch-point in Omarchy** (or route through a terminal `friday-ctl` extension).

This does not invalidate R5-2-5-2 (correctly frozen) — it only means the "chooser" layer is greenfield on the shell side, not a thin adapter over existing UI.

---

## What the shell-side landscape actually offers (established by this survey)

### Omarchy shell IPC model
- `bin/omarchy-shell <target> <method> [args]` forwards a call to the running Quickshell process via `qs ipc`. It never starts the shell.
- The top-level `shell` target (in `shell/shell.qml`) exposes `ping`, `summon`, `hide`, `toggle`, `call`, `rescanPlugins`, `reloadConfig`, `setPluginEnabled`, `listPlugins`. It also registers `image-selector`.
- **Plugins** register their own IPC targets and expose `open(payloadJson)` / `close()` lifecycle methods, driven by `shell.summon("<plugin-id>", payload)` / `shell.hide(...)`. The `omarchy.image-picker` panel is the canonical example: `IpcHandler { target: "image-selector" }` with `open()` building a payload and calling `shell.summon(...)`.
- A user hotkey (Hyprland `bindings.lua`, e.g. the commented-out `SUPER + SHIFT + S` capture binding) is the natural trigger for "explicit user action".

### The frozen `context.read` seam (from R5-2-5-2) the chooser will invoke
- Daemon ingress intercepts `method::CONTEXT_READ` (`"context.read"`) **after** the token gate, **before** generic dispatch.
- It is Screen-only, never ambient. Consent is a **per-use, request-attributable** opt-in: `params.consent: true` grants consent for that one read. Without it, `read()` is blocked at the ContextGate and the provider receives nothing (proven at the provider seam in R5-2-5-2).
- Response is surfaced as `CommandResult { ok, result }` and recorded to `state.json` as `ContextReadRecord { method, ms, ok, channel }`.
- Transport: WebSocket `127.0.0.1:18432`, token from `state.json`.

### The disclosure mechanism (frozen R5-2-4)
- `ContextGate::disclosure_for(channel, target) -> Option<&str>` returns `None` for local targets (nothing leaves) and the free-tier disclosure text for remote targets. This is the text that **must** be shown to the user **before** transmission.
- For Screen toward a remote provider this is non-empty — the chooser is the place to surface it.

---

## Candidate homes for the chooser

### Option A — First-party Omarchy FRIDAY panel (QML)
A `shell/plugins/...` FRIDAY panel registered by id, summonable via `shell.summon`, with an `open(payloadJson)` that shows the Screen + consent + disclosure, then `FabricProviderSeam`-style call to the daemon over the same WebSocket `friday-ctl` uses.
- **Pros:** native, matches image-picker precedent, can surface full disclosure UI, future R5-2-5-4 vision preview fits here.
- **Cons:** **most code** — new plugin manifest + QML panel + IPC target + shell config wiring. This is the "fancy UI" the user explicitly warned against for this slice.

### Option B — Minimal `friday-ctl` terminal subcommand (no QML)
Extend `friday-ctl` with an interactive (or simple two-step) path: user runs `friday-ctl context screen`, it prints the disclosure, asks for explicit `y/N` consent, then sends `context.read` with `consent: true` over the existing WebSocket.
- **Pros:** **smallest possible** — reuses the existing `friday-ctl` client/WebSocket/token plumbing entirely; zero QML, zero new IPC, zero Omarchy shell edits. Disclosure + explicit consent both fit naturally.
- **Cons:** not a GUI chooser; a hotkey can't easily pop a terminal chooser unless bound to a terminal-running command. "Chooser" is textual.

### Option C — Hotkey → `omarchy-shell` → summon a barebones minimal panel
Bind a Hyprland hotkey to `omarchy-shell <target> <method>` (a tiny new `friday.shell` IpcHandler) that summons the smallest possible confirmation surface.
- **Pros:** meets "FRIDAY hotkey → shell chooser" literally and matches the user's stated intended chain.
- **Cons:** still needs a new QML panel + IPC target (the bulk of Option A), just with minimal UI. It *is* the real GUI path, just trimmed.

---

## Recommendation for R5-2-5-3

**Option B is the disciplined first slice**, with **Option C held as the immediate successor (R5-2-5-3-1)** that swaps the terminal prompt for a hotkey-summoned minimal panel reusing the *same* frozen `context.read` path.

Rationale:
- The user's test for this slice is "brutally small, surgically confined." Option B **adds a single C command arm** (`context screen`) to an existing binary — no new IPC, no new UI architecture, no Omarchy shell edits, no `cargo fmt` exposure beyond `friday-ctl.rs`'s own file.
- It preserves the security invariant exactly: the disclosure is printed and the user must explicitly confirm before any `consent: true` is sent; without it, the frozen read path blocks and the provider sees nothing.
- It deliberately does **not** touch ContextGate, providers, capture, or the daemon's orchestration.
- Option C (hotkey + panel) is where the "FRIDAY hotkey → shell chooser" polish belongs, and it is small *because* it reuses the now-proven `context.read` seam — but it still requires new QML + IPC, so it should be its own subsequent zero, not bundled here.

**One thing the user also asked to confirm:** whether `friday-ctl` needs a minimal extension or whether the shell IPC can call `context.read` directly. Answer:
- The daemon's WebSocket already accepts `context.read` directly, so **any** client (shell plugin or `friday-ctl`) can call it — there is no shell-side gate. What's missing is a *user-triggered consent + disclosure front-end*. Option B provides it in `friday-ctl`; Option C would provide it in a panel. Neither needs changes to the daemon or the frozen seam.

---

## Boundary / scope guard for R5-2-5-3 (survey assertion)

In scope (Option B implementation, next zero):
- `friday-ctl.rs`: add `context screen` subcommand → print disclosure → explicit y/N consent → send `context.read` with `consent: true`; error paths surface the frozen `ContextReadError` text.
- Rust formatting scoped strictly to `friday-ctl.rs` (no workspace `cargo fmt` — the documented contamination guard).

Out of scope (deferred):
- ❌ No QML panel / Omarchy shell / `IpcHandler` edits (future R5-2-5-3-1 hotkey panel).
- ❌ No capture implementation (collector stays frozen R5-2-5-1).
- ❌ No ContextGate changes.
- ❌ No provider changes.
- ❌ No ambient/automatic context.
- ❌ No new IPC architecture.

**Frozen file:** this survey (`R5-2-5-3-0-SHELL-CHOOSER-SURVEY.md`). No QML or `friday-ctl` changes until this is approved.

---

## Suggested next slice (pending approval)
`R5-2-5-3` — implement the minimal `friday-ctl context screen` consent+disclosure reader, proving at the `friday-ctl`/daemon boundary that unconsented requests never reach the provider.
