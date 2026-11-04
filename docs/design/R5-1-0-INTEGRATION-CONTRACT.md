# R5-1-0: FRIDAY Integration Contract

> Read-only survey (2026-08-29) of the FRIDAY Rust runtime side, done before
> writing any shell plugin or daemon work. The contract that frames R5-1-1
> (boot → daemon → shell vertical slice) and all later OS integration.
>
> The runtime already contains most of what a daemon needs; this contract
> defines what is reused as-is versus what is genuinely greenfield, and fixes
> the exact surface between the FRIDAY runtime and the Quickshell shell.

## The central finding

The runtime is a *library of proven machinery* where the **entire daemon
harness already exists** (`friday-mobile-daemon`) but only runs **goal-reactive
not goal-initiating**, and there is **zero Linux integration** (no signals, no
D-Bus notifications, no desktop lifecycle). So the contract's job is narrow:
take the existing daemon harness, make it a Linux graphical-session service
that both *owns the loop* and *serves the shell* — reusing Omarchy's existing
shell mechanisms exactly where they fit, with no new IPC framework, no second
event bus, no second shell architecture.

## [EXISTS] — reused as-is from the runtime

- **`GoalRunner` + `Executive` + real adapters** — canonical live-host wiring
  `with_fabric_and_memory(bus, fabric, journal, voice)` at
  `services/mobile/src/goal_runner.rs:1033-1052`, composing
  `Executive::new().with_investigation_engine(...).with_episode_journal(journal)`
  plus memory + 9 real evidence adapters (filesystem, web, docs, git,
  codeintel, webagent, terminal, browser, windows) at `goal_runner.rs:1065-1230`.
- **The daemon harness** — `friday-mobile-daemon`
  (`services/mobile/src/bin/friday-mobile-daemon.rs:40-159`): `Friday` core +
  `EventBus` + `WsServer` (18432) + `GoalRunner` + `CommandGateway` +
  `VoiceIntake`. Reused; goal-initiation is the missing part.
- **WebSocket transports** — workspace `tokio-tungstenite = "0.24"`
  (`Cargo.toml:50`); bidirectional WS 18432 (`mobile/src/transport.rs`),
  read-model WS 18433 + HTTP metrics 18434 (`services/bridge/src/server.rs`).
- **Read-model / DTO surface** — `DashboardDto`, `ExecutivePhaseStateDto`,
  `InvestigationGraphDto`, `EvidenceTrailDto`, `MetricsReportDto`
  (`services/bridge/src/protocol.rs`); `RuntimeSource` plug-in seam
  (`services/bridge/src/source.rs:87-98`).
- **Command vocabulary + lifecycle events** — `planner.goal`, `task.cancel`,
  `task.pause`, `task.approve`, `system.info` (`mobile/src/commands.rs:21-27`);
  `goal.*` lifecycle events; `notifications` topic.
- **Per-cycle cognitive state stream** — `CognitiveBlackboard` broadcast
  (`workspace.rs:18-37`): `active_goal`, `active_plan`, `attention`,
  `world_state`, `execution`, `reflection`, etc.
- **Voice** — `VoiceIntake → plan.goal` (session `"voice"`, same bus as a typed
  goal), config-gated by `FRIDAY_VOICE` + `FRIDAY_WHISPER_*`
  (`mobile/src/voice.rs:211-254`). Voice is just another `plan.goal` producer.

## [GREENFIELD] — the actual new work

- **Linux-graphical daemon that owns goal initiation** (today the loop only
  reacts to `plan.goal`).
- **Loopback-only WS ingress + per-session token auth** for the shell
  (mobile WS pair-auth `transport.rs:101-295` is overkill for localhost; the
  bind address itself is the trust boundary, plus a token from the state dir —
  mirroring Omarchy's shell path-traversal validation precedent).
- **Thin `state.json` snapshot** for boot-time + widget fallback (mirrors the
  Omarchy agents plugin: daemon writes JSON, plugin watches via FileView).
- **SIGTERM / sd_notify / graceful shutdown** — `Executive::cancel` +
  journal flush + WS close; **crash-restart** under the user service;
  **recovery bootstrap** on restart (re-read state.json, re-establish WS).
- **The non-Windows observer seam** — `live_observer` (mic→Observe fold) is
  `#[cfg(windows)]`-only (`goal_runner.rs:1239-1253`); on Linux voice enters
  via `VoiceIntake → plan.goal` instead.

## Reused from Omarchy (not rebuilt)

- **Notifications** — FRIDAY runtime has ZERO D-Bus/notify-rust. Send desktop
  notifications through **Omarchy's existing in-process freedesktop D-Bus
  NotificationServer** (`shell/services/Service.qml:931-943`) / the
  `omarchy-notification-send` route, NOT a second notification daemon.
- **File-backed state precedent** — daemon writes `state.json`, plugin watches
  (the `agents` plugin pattern: `~/.local/state/omarchy/agents/usage/`).
- **Session lifecycle target** — user systemd unit `WantedBy=graphical-session.target`
  alongside Omarchy's existing 6 enabled session units.

## The contract (tiny by design)

```text
Omarchy substrate                 FRIDAY (project-friday)
────────────────────              ────────────────────────
Quickshell shell          ◄──────  friday daemon (reuse friday-mobile-daemon
  friday.shell plugin                harness + GoalRunner + real adapters)
       │                              │
       │   file-backed heatbeat       ├── state.json ──► (thin per-cycle snapshot
       │   + per-session token        │                    for boot-time + fallback)
       ▼                              ▼
  ws://127.0.0.1:18433 (read-model)  ◄── DashboardDto / ExecutivePhase / Graph / Evidence
  ws://127.0.0.1:18432 (bidir)       ◄── planner.goal / task.* / goal.* / notifications
       │
       │   notifications ──► Omarchy in-process D-Bus NotificationServer (reuse,
       │                      NOT a second daemon)
       │
       ▼
  Omarchy theme engine (shell push)  ◄── status / current task / cognition / voice state
```

**Reused as-is:** `GoalRunner`/`Executive`/adapters, WS 18432+18433, DTO
surface + `RuntimeSource` seam, command vocabulary + `goal.*` + `notifications`
topics, voice `VoiceIntake`, Omarchy's in-process D-Bus NotificationServer.

**Greenfield:** goal-owning Linux daemon, loopback+token auth, thin
`state.json`, SIGTERM/sd_notify/graceful shutdown, crash-restart, non-Windows
observer seam.

## Phase plan (from the user)

- **R5-1-0** integration contract — this document
- **R5-1-1** boot → daemon → shell (smallest vertical slice)
- **R5-1-2** live cognitive UI
- **R5-1-3** controlled desktop command
- **R5-2** AI account/session research (web-session provider is a *provider*,
  not FRIDAY's foundation; if a service's web UI changes, FRIDAY loses one
  provider, not its brain)
- **R5-3** OS-level context (screen/windows/clipboard/files/terminal/notifications/
  mic/keyboard/apps) with explicit permission boundaries
- **R5-4** FRIDAY experience (login/boot/shell/bar/launcher/notifications/voice/
  command palette/cognition panel/themes/identity)
- **R5-5** standalone ISO (fork/adapt `omarchy-pkgs` or own packaging — decided
  only then)

The first real FRIDAY OS milestone:

> **Boot FRIDAY → daemon starts → shell discovers `friday.shell` → FRIDAY
> displays live cognitive state → one controlled command from the desktop.**
