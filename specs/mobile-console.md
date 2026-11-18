# FRIDAY Mobile Console — M6

**Status:** 🟢 M6.1 done · M6.2a done · M6.2b client **builds + lints green**
(protocol `1.0`, explicit connection states) · **Client Qualification pending**
(on-device Demo 001)
**Owner:** Reliability Engineering
**Rule:** Evidence-based. The phone is a *remote terminal* for FRIDAY running on
Windows — **never** a second AI. The J7 is display + microphone + speaker +
camera + notifications. All intelligence stays on the PC. The Mobile Protocol is
now a **versioned API** (`clients/README.md`); clients are qualified like any
other subsystem (`specs/client-qualification.md`).

## Why now

Every subsystem so far has been *built*. The mobile console is the **first real
client** of FRIDAY: it exercises the Planner, AgentKernel, Windows Bridge,
Inference Fabric, Memory, telemetry and recovery **every day**. That usage is the
strongest qualification signal we have.

## Architecture

```
          Windows PC — FRIDAY daemon
   ┌──────────────────────────────────────┐
   │ Planner · AgentKernel · Windows Bridge│
   │ Inference Fabric · Memory · Recovery   │
   │            services/mobile  ◄──────────┤  WebSocket + HTTPS
   └──────────────────┬───────────────────┘
                       │  (mobile service is the ONLY phone-facing surface)
              WebSocket (realtime) + REST (login/config)
                       │
              ┌────────▼─────────┐
              │  Samsung J7       │
              │ FRIDAY Launcher   │
              │ Dashboard/Tasks/  │
              │ Terminal/Files    │
              └──────────────────┘
```

The phone talks **only** to `services/mobile`. It never touches the Planner
directly. The mobile service relays commands inward and events/telemetry
outward, reusing `friday-capabilities` `ServiceStatus` / `Telemetry` and the
core event bus for the dashboard.

## Communication

- **WebSocket** for realtime: tasks, logs, streaming, notifications, telemetry,
  auth handshake, command/result.
- **REST** only for: login, config download, asset downloads.
- JSON wire protocol (`protocol.rs`), tagged `type` for routing.

## Authentication (Alpha = pairing code)

```
Code on the PC  →  Type it on the phone  →  Save Token  →  Reconnect
```

1. The PC shows a short `pairing_code` (8 digits, valid 2 minutes). It is shown
   on the PC only: the server never sends it to a phone.
2. An unpaired phone sends `Hello`; the server answers `AuthChallenge { challenge }`,
   a fresh random challenge for that connection.
3. The user types the code on the phone, which sends `AuthResponse { code, signature }`
   where `signature = sha256(challenge + code)`.
4. Server validates, issues a `token` (uuid), phone persists it.
5. Reconnect sends `Hello { token }`; server validates and skips pairing.

Five wrong answers discard every pending code and block new ones for five
minutes, so the short code can't be guessed online.

> Alpha note: the link is a plain WebSocket, not encrypted, and the phone sends
> the code across it. Someone who can watch the network (shared Wi-Fi) can read
> the code while you pair. Pair at home or on a network you trust until the link
> is encrypted (a TLS or key-exchange follow-up, not Alpha).

## M6 roadmap

### M6.1 — Mobile Transport (this slice)
- `services/mobile` crate: WebSocket server, framed JSON protocol, QR-pair auth,
  heartbeat, notification/event broadcast, command ingress.
- Unit + integration tests (spin a local WS server, full auth handshake, command
  round-trip, event broadcast).
- Reuses `friday-capabilities::orchestrator::{ServiceStatus, Telemetry}` and
  `friday_capabilities::model::Health` for the dashboard payloads.

### M6.2 — FRIDAY Launcher (split into disciplined slices)

#### M6.2a — Live daemon integration ✅ DONE
- `MobileSession` abstraction (`session.rs`): `phone_id`, `authenticated`,
  `connected`, `subscribed_events`, `pending_approvals`, `active_tasks`,
  `capabilities{voice,camera,notifications,speaker,microphone}`. `SessionRegistry`.
- `BusBridge` (`bridge.rs`): subscribes the core `EventBus` and forwards every
  event as a `ServerMessage::Event` (grouped topic: `tasks`/`plans`/`notifications`
  etc.), `translate()` maps planner kinds → friendly topics. Never polls.
- `SystemProbe` (`probe.rs`): real host CPU/RAM/Disk telemetry via `sysinfo`,
  published onto the bus (no fake numbers).
- `CommandGateway` (`commands.rs`): structured commands
  (`planner.goal` with optional `web: true`, `task.cancel`, `task.pause`, `task.approve`, `system.info`)
  → real bus `Event`s, not stubs.
- `WsServer` now carries `sessions: Arc<SessionRegistry>`; inserts/updates/removes
  sessions on connect/auth/command/disconnect; tracks `active_tasks` /
  `pending_approvals`.
- `bin/friday-mobile-daemon.rs`: wires `Friday` runtime → bus → probe + bridge +
  command gateway + WS server.
- **End-to-end test** `live_integration_real_telemetry_and_command` proves: real
  `telemetry.system` from the bus reaches the phone, and a `planner.goal` command
  round-trips back onto the bus and a `CommandResult` returns to the phone.

#### M6.2b — Android Launcher (scaffold) ✅ BUILDS + LINTS GREEN (APK produced)
- Kotlin + Compose client under `clients/android/`. Architecture split into
  `protocol/ network/ data/ domain/ ui/ notifications/` — only protocol DTOs,
  never FRIDAY internals. Covers M6.2b.1–b.5 + launcher + lock screen + M6.2.6
  notifications. See `clients/android/README.md`.
- **Protocol `1.0`**: handshake exchanges `protocol` in `Hello` + `Welcome`
  (`PROTOCOL_VERSION` in `protocol.rs`); clients announce version, server rejects/
  negotiates incompatible ones. Versioned deliberately, never broken casually.
- **Explicit connection states**: `Disconnected · Connecting · Authenticating ·
  Connected · Degraded · Reconnecting`. Failures move to `Reconnecting` (bounded
  auto-retry). The launcher never guesses connection state.
- **Task detail reordered chronologically**: Goal → Planner → Execution →
  Recovery → Evidence → Logs (Evidence outranks raw Logs).
- **Build-verified 2026-07-15**: `assembleDebug` BUILD SUCCESSFUL (Gradle 8.2 +
  JDK 17 + SDK platform-34/build-tools 34.0.0), `app-debug.apk` produced,
  `lintDebug` 0 errors. Compose enabled via
  `composeOptions { kotlinCompilerExtensionVersion = "1.5.10" }` (the
  `org.jetbrains.kotlin.plugin.compose` plugin only exists from Kotlin 2.0). See
  `specs/client-qualification.md` for the full verification record.
- **Qualification**: gated by `specs/client-qualification.md` (Build · APK · Lint ·
  Instrumentation · Protocol · Manual Demo) and **Demo 001**. Not merged until the
  five merge verifications pass on real hardware.

#### M6.2c — Live command execution
- Wire launcher buttons (pause/cancel/approve/goal) to real gateway commands.

#### M6.2d — Notifications
- `notification.*` events surfaced on the phone; phone notification shade bridge.

#### M6.2e — Approval dialogs
- `task.approve` request/response UI; pending_approvals tracked in session.

#### M6.2.6 — Notification actions
- Failed-task notification with [Open]/[Retry]/[Ignore] emitting real protocol
  commands (already scaffolded in the client; wired to `M6.2c`/`M6.2e` methods).

#### M6.2.7 — Session Transfer (pre-voice)
- Task in flight on Desktop continues seamlessly on Phone and back. Makes FRIDAY
  feel like one system. A protocol capability (`session.transfer`), not client
  logic. Logged before M6.3.

### M6.3 — Voice Remote
- Push-to-talk; record on J7, stream audio to PC; PC runs Whisper + VAD + LLM.

### M6.4 — Vision Remote
- Camera streaming / screenshot → PC vision model → Planner.

### M6.5 — Remote Control
- Approvals, Terminal (persistent TerminalAdapter session), Files (Filesystem
  Adapter), Recovery, Benchmarks.

## What NOT to do

- Do **not** run AI / STT / vision on the J7. It is a thin client.
- Do **not** expose Android to the user (launcher = "FRIDAY OS" skin).
- Do **not** let the phone call Planner/Inference directly — everything via
  `services/mobile`.

## Acceptance criteria (M6.1 exit)

- [x] `friday-mobile` builds clean (fmt / clippy `-D warnings` / tests).
- [x] Wire protocol documented (this file + `protocol.rs` doc-comments).
- [x] Full QR-pair handshake testable without a phone (integration test).
- [x] Command ingress + event broadcast round-trip verified by test.
- [x] Server degrades gracefully when a client disconnects / sends garbage.

## Acceptance criteria (M6.2a exit)

- [x] `services/mobile` subscribes the real core `EventBus` (no polling).
- [x] Real `telemetry.system` (sysinfo) reaches a connected phone via the bridge.
- [x] Structured commands (`planner.goal`, `task.*`, `system.info`) emit real bus
      events through `CommandGateway`.
- [x] `MobileSession` lifecycle tracked in `SessionRegistry` across connect/auth/
       command/disconnect.
- [x] End-to-end test proves telemetry-in + command-round-trip without a phone.

## Acceptance criteria (M6.2b exit → 🟢 Client Qualified)

- [x] Android project builds clean (`assembleDebug` BUILD SUCCESSFUL; release not
       yet configured).
- [x] `./gradlew lint` clean (errors zero).
- [ ] At least one instrumentation/smoke test exercises launch + handshake.
- [x] Client DTOs verified against `services/mobile/src/protocol.rs` (protocol `1.0`).
- [ ] **Demo 001** performed on the J7 against `friday-mobile-daemon`.
- [ ] The five merge verifications recorded: APK builds · QR pairing works · live
       telemetry updates · `Open Notepad` works · notification `Retry` executes.
- [ ] Client UI frozen after Demo 001 (no new screens until desktop matures).

See `specs/client-qualification.md` and `clients/README.md`.
