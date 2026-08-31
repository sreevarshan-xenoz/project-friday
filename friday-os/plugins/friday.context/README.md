# friday.context — minimal consent dialog for sharing desktop context (R5-2-5-3-1)

A hotkey-summoned Omarchy `panel` plugin that asks the user for explicit
permission before the FRIDAY assistant reads the Screen. It is FRIDAY-owned and
**lives outside the frozen `omarchy/` tree**; it is delivered to the shell only
as a third-party plugin drop-in at the user path (the same boundary `friday.shell`
already establishes).

## What it is

A tiny dialog: title, a plain-language description of what will be shared, and
two buttons — **Cancel** and **Share**.

```
FRIDAY hotkey
      ↓
omarchy-shell shell toggle friday.context
      ↓
friday.context panel   [ Cancel ]  [ Share ]
                                  ↓
                    friday-ctl context screen --yes
                                  ↓
                         context.read { consent: true }
                                  ↓
                    ContextGate → EgressGate → provider
```

On **Share** the panel runs `friday-ctl context screen --yes` (fire-and-forget
via `Quickshell.execDetached`). That reuses the audited R5-2-5-3 CLI client,
which sends `context.read` with a per-use `consent: true`.

## The security boundary is NOT in QML

This panel is deliberately **not** a security boundary. It only expresses the
user's intent — "Share was pressed". The authoritative chain is Rust-only:

- **ContextGate** — decides whether the channel is authorized.
- **EgressGate** — decides whether the payload is safe to leave.
- **Provider** — receives only what survived both gates.

The `consent: true` flag is required by the frozen R5-2-5-2 seam and enforced by
the gate; the QML merely conveys that the user consented. Editing this QML can
never weaken the model — it can only fail to express consent.

## Why it avoids first-party module namespaces

This third-party plugin imports only Quickshell core + QtQuick (`QtQuick`,
`QtQuick.Controls`, `QtQuick.Layouts`, `Quickshell`, `Quickshell.Wayland`) and
**never** `qs.Ui` / `qs.Commons`. Those are first-party module namespaces that a
plugin delivered to `~/.config/omarchy/plugins/` cannot rely on resolving. This
mirrors the shipped `friday.shell` plugin, which also draws its widgets from
`QtQuick.Controls` rather than the shell's private kit.

## Install

```bash
shell_json="$HOME/.config/omarchy/shell.json"

install -d "$HOME/.config/omarchy/plugins"
cp -r friday-os/plugins/friday.context "$HOME/.config/omarchy/plugins/friday.context"

# Enable the third-party panel in the user shell config (id must not start
# with the reserved `omarchy.` prefix).
#   "$shell_json" gets:  { "version": 1, "plugins": [ { "id": "friday.context" } ] }
omarchy-shell shell rescanPlugins
```

Hotkey in the user Hyprland bindings override (`~/.config/hypr/bindings.lua`):

```lua
o.bind("SUPER + CTRL + F", "FRIDAY context", "omarchy-shell shell toggle friday.context")
```

No edits to `shell/shell.qml`, the plugin registry, or any first-party config are
required — the generic `shell toggle/summon/hide` IPC handles any enabled
third-party panel id.

## Layout

```
manifest.json  — third-party `panel` plugin descriptor (schemaVersion 1, id friday.context)
Panel.qml      — the consent dialog + Share/Cancel lifecycle
```

## Verification status

- The `friday-ctl context screen --yes` seam (the only Rust-side change) is
  **fully verified on this Windows dev box**: consent is per-use, `--yes` skips
  only the terminal prompt (never the gate), and the mandatory `consent: true`
  is still what the daemon requires.
- The QML **cannot be visually executed on this Windows dev box** (no Quickshell
  shell), so `friday.context` is authored against the real Omarchy plugin
  contract and its manifest parses, but on-screen rendering and the hotkey
  summon are verified on a real Omarchy session — see R5-2-5-4, the first real
  Omarchy boot test. `omarchy/` remains untouched (pristine at `0b3f1b7e`).
