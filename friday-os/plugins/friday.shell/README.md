# friday.shell — FRIDAY bar-widget for the Omarchy shell

The GRID/FRIDAY cognitive-state widget as a native Omarchy `bar-widget` plugin.
It is FRIDAY-owned and **lives outside the frozen `omarchy/` tree**; it is
delivered to the shell only as a plugin drop-in at the user path.

## What it is

- A read-model over the FRIDAY Linux daemon's file-backed contract
  (`~/.local/state/friday/state.json`): daemon status, current goal + phase,
  cognition slots (voice), and host telemetry (CPU/memory).
- A single controlled command (`planner.goal`) to the daemon, issued from the
  desktop through the `friday-ctl` CLI.
- An IPC surface at the `friday.shell` target (`status`, `refresh`, `goal`).

It reuses the Omarchy plugin contract exactly: `manifest.json` (bar-widget,
`Panel.qml` entry point), `Panel` base class with `moduleName`/`ipcTarget`/
`manageIpc:false`, `FileView` watching, and `BarIconButton`/`KeyboardPanel`
widgets. No change inside `omarchy/` is required.

## Layout

```
manifest.json   — bar-widget plugin descriptor (schemaVersion 1, id friday.shell)
FridayState.qml — headless FileView read-model over state.json
Panel.qml       — the bar widget + IpcHandler + command entry
```

## Install

```bash
install -d "$HOME/.config/omarchy/plugins"
cp -r friday-os/plugins/friday.shell "$HOME/.config/omarchy/plugins/friday.shell"
```

The plugin is then discoverable as a normal Omarchy bar widget and can be
added to the bar like any other. `moduleName`/`ipcTarget` are `friday.shell`,
so `omarchy` → IPC can target it once loaded.

## The controlled command

The widget never talks to the daemon itself. It shells out to `friday-ctl`
(the CLI over the daemon's loopback, token-gated WebSocket) and renders the
result. This keeps the single controlled-command surface in one audit-traceable
place — the `planner.goal` method — and matches the R5-1-1 contract.

Daemon prerequisites (documented in the integration contract):

- `friday-linux-daemon` running under the user session, writing
  `~/.local/state/friday/state.json`.
- `friday-ctl` on PATH (or the `ctl.command` manifest default overridden).

## Status of this slice (R5-1-1)

Per the R5-1-0 integration contract, the runtime side of this vertical slice is
complete and proven headlessly:

- `friday-linux-daemon` — token-gated WebSocket ingress, `state.json` heartbeat.
- `shell_state` — pure `apply_event` state-transition core (unit-tested).
- `friday-ctl` — the one command from the desktop (build + live-tested end to
  end here: daemon accepted the goal, `state.json` echoed it).
- `tests/shell_contract.rs` — end-to-end contract test over the live socket.

This QML widget is authored against the real Omarchy plugin contract and its
manifest parses, but it **cannot be visually executed on the Windows dev box**
(the Quickshell shell does not run here). It is reviewed for structural
correctness against the frozen `agents` plugin; visual verification is deferred
until an Omarchy session is available (R5-1-2/R5-1-3).
