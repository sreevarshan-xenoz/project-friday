# R5-2-5-3-1 — Omarchy FRIDAY Context Chooser (IMPLEMENTATION FREEZE)

**Status:** FROZEN implementation of the Omarchy consent dialog for sharing the Screen with FRIDAY.
**Inputs:** frozen R5-2-5-3-1-0 survey (`612b954`), frozen R5-2-5-3 (`71d0986`), frozen R5-2-5-2 (`26a0e41`), `dev @ 612b954`.
**This freeze:** a tiny hotkey-summoned `friday.context` panel (Omarchy) that expresses the user's Share/Cancel intent and drives the proven `friday-ctl context screen --yes` path. **No QML security logic, no `context describe`, no daemon redesign, no second IPC.**

---

## Boundary discovery (corrects the survey's assumption)

R5-2-5-3-0 concluded `friday.shell` "did not exist" because it searched only the `omarchy/` tree. **That was incomplete.** A FRIDAY-owned adaptation layer already exists at **`friday-os/plugins/friday.shell/`** (bar-widget, R5-1-2/3), tracked in the FRIDAY root repo and delivered to `~/.config/omarchy/plugins/` — **outside** the frozen `omarchy/` checkout.

Established boundary (verified, reproducible):
- **`omarchy/` is a pristine independent git repo** on branch `quattro` at upstream commit **`0b3f1b7e`**. It is not a submodule of the FRIDAY repo (no `.gitmodules`; the root tracks 0 omarchy files). It must stay untouched.
- **The FRIDAY OS adaptation layer lives in `friday-os/plugins/`** (FRIDAY repo, third-party plugin naming `friday.*`), provisioned by copying into `~/.config/omarchy/plugins/<id>/` + enabling in `~/.config/omarchy/shell.json` `plugins[]` + a hotkey in `~/.config/hypr/bindings.lua`.

This freeze **relocated** an earlier mis-placed panel (originally dropped under `omarchy/shell/plugins/` with the reserved `omarchy.*` id) into the proper adaptation layer as **`friday.context`**, and **restored `omarchy/` to pristine** (git status clean). `0b3f1b7e` is reproducible.

---

## What was built

### 1. Rust seam (only Rust change) — `services/mobile/src/bin/friday-ctl.rs`
`friday-ctl context screen --yes`: when the caller has already collected explicit consent (the panel's Share button), `--yes` skips only the **TTY consent prompt**. The mandatory `consent: true` is still sent and still enforced by the frozen ContextGate — `--yes` never weakens the gate, it just reflects that the UI did the prompting. One-line semantics, scoped to this file, no `cargo fmt`.

### 2. Adaptation plugin — `friday-os/plugins/friday.context/`
- `manifest.json` — third-party `panel` kind, id `friday.context` (valid third-party id: not in the reserved `omarchy.` namespace), `entryPoints.panel: "Panel.qml"`, `keepLoaded: true`.
- `Panel.qml` — a minimal centered dialog (title, plain-language description, **Cancel** / **Share**). Share → `Quickshell.execDetached(["friday-ctl","context","screen","--yes"])`. Cancel/click-outside → `dismiss()` → `shell.hide(id)`. Uses **only** `QtQuick` / `QtQuick.Controls` / `Quickshell` / `Quickshell.Wayland` — never `qs.Ui`/`qs.Commons`, because those first-party module namespaces cannot be relied on from `~/.config/omarchy/plugins/` (mirrors the shipped `friday.shell`).
- `README.md` — ownership boundary, install/enable/hotkey steps, and the explicit "QML not verified on Windows" caveat.

### 3. Provisioning (documented in README, not applied here — no Omarchy runtime on this box)
```bash
cp -r friday-os/plugins/friday.context "$HOME/.config/omarchy/plugins/friday.context"
# ^ plus: enable { "id":"friday.context" } in ~/.config/omarchy/shell.json plugins[]
o.bind("SUPER + CTRL + F", "FRIDAY context", "omarchy-shell shell toggle friday.context")  # ~/.config/hypr/bindings.lua
```

---

## Security contract (unchanged and authoritative)

The QML is **not** a security boundary:
```
friday.context — only "Share was pressed"
      ↓
friday-ctl context screen --yes  →  context.read { consent: true }
      ↓
ContextGate  — decides channel authorization
      ↓
EgressGate   — decides egress safety
      ↓
Provider     — receives only what survived both gates
```
Editing QML can never silently disable the security model — it can only fail to express consent.

---

## Verification

| Gate | Result |
|---|---|
| `cargo clippy -p friday-mobile --bin friday-ctl -- -D warnings` | Clean (only Rust change). |
| `cargo test -p friday-mobile` | 79 lib + 1 integration pass (no regression). |
| `cargo build` (workspace) | Clean. |
| `friday-ctl context screen --yes` (no daemon) | Skips prompt, reaches token resolution ("daemon not running") — seam proven; the mandatory `consent: true` is unchanged. |
| `friday.context/manifest.json` | Valid JSON; schema-conformant (schemaVersion 1, id, kinds, entryPoints.panel present, valid non-reserved id `friday.context`). |
| `omarchy/` | **Pristine** (git status clean at `0b3f1b7e`). |

**Honest caveat:** the Rust seam is fully verified on Windows; the QML panel and its hotkey summon **cannot be visually executed here** (no Quickshell shell). Rendering + the live hotkey → Share → vision-provider loop are verified on a real Omarchy session — that is **R5-2-5-4**, the first real Omarchy boot test. Until then we have proven **screen-context transport**, not "FRIDAY can see your screen."

---

## Scope guard

Touches only:
- `services/mobile/src/bin/friday-ctl.rs` (the `--yes` seam, one file).
- `friday-os/plugins/friday.context/` (new adaptation plugin).

Does **not** touch: the frozen `omarchy/` checkout, `friday.shell`, ContextGate, EgressGate, providers, daemon, protocol, or `shell.qml`. No `context describe`. No ambient capture. No new daemon IPC. No vision abstraction.

Next stone: **R5-2-5-4** — real Omarchy boot + actual `grim` capture + real vision provider, answering "can FRIDAY see your screen and reason about it?".
