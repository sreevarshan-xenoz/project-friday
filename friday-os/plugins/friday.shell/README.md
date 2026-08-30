# friday.shell — FRIDAY, a calm cognitive-status bar-widget for the Omarchy shell

The GRID/FRIDAY status widget as a native Omarchy `bar-widget` plugin. It is
FRIDAY-owned and **lives outside the frozen `omarchy/` tree**; it is delivered
to the shell only as a plugin drop-in at the user path.

## What it is (R5-1-2)

A **passive, observe-only** surface that reflects FRIDAY's genuine state — it
does not command the runtime, and it never invents a metric.

- A glanceable bar dot: quiet truth, three states only — `●` idle/online,
  `●` working (a goal is genuinely in flight), `○` unavailable (daemon down or
  state file missing).
- A rich panel exposing only what the daemon actually reports: daemon status,
  the current activity, the **last goal's genuine outcome** (result/error,
  phases completed, route), voice, host telemetry, and the last accepted
  command echo.

It deliberately does **not** show fake AGI meters, "confidence 97%", or a
decorative brain animation. If FRIDAY does not produce a measurement, the
widget says nothing — exactly the ACOS calm/anti-fabrication direction.

## Why it is defensive

This plugin runs **inside the shell host** and is not sandboxed, so every
accessor in `FridayState.qml` is guarded. A missing `state.json`, malformed
JSON, an empty/partial document, or a vanished field degrades to a safe value
(`FRIDAY not running` / stale) — never a property error that kills the widget.

## The read model

`state.json` is the contract, and only the daemon writes it. The widget only
observes it via `FileView` (reacting to the daemon's atomic tmp+rename writes):

```
GoalRunner / Executive
        ↓  bus events
   shell_state.apply_event   ← records only genuine fields
        ↓
   state.json
        ↓
   FileView
        ↓
   FridayState.qml (defensive parse)
        ↓
   friday.shell panel
```

`apply_event` (in `services/mobile/src/shell_state.rs`) maps the real event
set into a UI-complete document: `plan.goal`/`goal.started` open a `current`
goal; the terminal events (`goal.completed|failed|cancelled|paused|
recoverable`) close it into `last_goal` carrying only the outcome detail the
runtime attached (`phases`, `result`, `error`, `strategy`,
`evidence_verified`). It never manufactures a field.

## Layout

```
manifest.json   — bar-widget plugin descriptor (schemaVersion 1, id friday.shell)
FridayState.qml — defensive FileView read-model over state.json
Panel.qml       — the glanceable bar dot + rich panel + IpcHandler
```

## Install

```bash
install -d "$HOME/.config/omarchy/plugins"
cp -r friday-os/plugins/friday.shell "$HOME/.config/omarchy/plugins/friday.shell"
```

The widget is then discoverable as a normal Omarchy bar widget and can be added
to the bar. Its `moduleName`/`ipcTarget` is `friday.shell`, exposing
`open`/`close`/`toggle`/`refresh`/`status`.

## R5-1-2 status

The read model is hardened and proven end-to-end headlessly (live daemon:
`goal.completed` → `last_goal.outcome/phases/strategy` reflect the Executive's
real cycle; a goal with no captured output correctly reports no result). The
R5-1-1 contract test and the `shell_state` unit suite are green.

The QML cannot be visually executed on this Windows dev box (no Quickshell
shell), so the widget is authored against the real Omarchy plugin contract and
its manifest parses, but on-screen rendering is verified on a real Omarchy
session in the dedicated visual-verification step. Command entry is explicitly
**not** part of R5-1-2 — that is R5-1-3.
