# frontend/

Desktop UI built with **React + TypeScript + Vite**. Designed to eventually run
inside **Tauri** for native desktop integration.

## Design philosophy

FRIDAY's UI is **not** a chatbot interface. It is a three-layer living
environment:

1. **Ambient Mode** — idle screen showing system status, time, greeting, tasks.
2. **Conversation** — card-based responses instead of walls of text. Each FRIDAY
   response becomes a structured card with actions.
3. **Workspace** — per-project views with memory, files, tasks, terminal, and logs.

Visual style: dark-first, soft glows, lots of spacing. Between Apple Intelligence,
Linear, Arc Browser, and Iron Man HUD. No flashy RGB.

## Architecture

```
src/
├── tokens.css          CSS custom properties (design tokens)
├── tokens.ts           TypeScript constants for programmatic use
├── index.css           Global styles, fonts, animations
├── protocol/           WebSocket client + typed Mobile Protocol DTOs
│   ├── protocol.ts     Wire types mirroring protocol.rs exactly
│   ├── client.ts       FridayClient class (connect, auth, events, commands)
│   └── index.ts        Barrel export
├── hooks/              React hooks
│   ├── useFriday.ts    FridayContext provider + useFriday()
│   └── useProtocol.ts  useConnection(), useEvents(), useCommandResults()
├── components/         Reusable UI primitives
│   ├── Card            Animated card with glow variants
│   ├── StatusPill      Color-coded health indicator
│   ├── MetricBar       CPU/RAM/Disk progress bar
│   ├── ChatBubble      User/FRIDAY message bubble
│   ├── ResponseCard    Structured FRIDAY response with actions
│   ├── Button          Primary/secondary/ghost/danger variants
│   ├── Input           Styled input with icon support
│   ├── Terminal        Terminal emulator with colored output
│   └── SectionLabel    Uppercase section header
├── screens/            Page components
│   ├── HomeScreen      Ambient dashboard (time, greeting, resources, tasks)
│   ├── ChatScreen      Card-based conversation with events
│   ├── ProjectsScreen  Workspace view with per-project tabs
│   ├── MemoryScreen    Timeline of FRIDAY's experience
│   ├── TerminalScreen  Command console with live event stream
│   └── SettingsScreen  Connection config and status
├── NavShell.tsx        Sidebar navigation + content area + ambient glow
├── App.tsx             Root provider + client initialization
└── main.tsx            Entry point
```

## Protocol layer

The `protocol/` module mirrors `services/mobile/src/protocol.rs` and
`clients/android/.../protocol/ProtocolModels.kt` **exactly**:
- Tagged JSON (`"type"` discriminator, `snake_case` variants)
- Handshake: `Hello` → `Welcome` → optional `AuthChallenge` → `AuthComplete`
- Commands: `ClientMessage::Command` → bus → `ServerMessage::CommandResult`
- Events: `ServerMessage::Event` with `_kind`/`_source` bridge tags
- Current version: **`1.0`**

Clients never import FRIDAY internals — only protocol DTOs.

## Engineering Console (Phase 1)

The `src/console/` module is the **FRIDAY Control Center** — the engineering
UI that consumes the frozen Sprint 5/6 read models. It is deliberately UI-only:
the screens know nothing about the planner, the runtime, the ledger, or the
experience engines — only the read-model DTOs.

```
src/console/
  types.ts         Shape-accurate TS mirrors of the frozen Rust contracts
                   (contract.rs + experience/src/lib.rs) + the console DTOs
                   (DashboardDTO, WorldState, RuntimeStatus, ConsoleFrame)
  mock.ts          Deterministic synthetic ledger + derived read models
                   (LCG-seeded, mirrors VP-004 + the real engines)
  dataSource.ts    ConsoleDataSource interface + implementations
                   ├── MockConsoleDataSource   (simulated ticks, bridge 'mock-friday')
                   └── RuntimeConsoleDataSource (ReadModelBridge @ ws://127.0.0.1:18433)
  store.ts         Reactive controller + useConsole() (subscribes + merges frames)
  ui.tsx / ui.css  Console primitives (Panel, Stat, Bar, Chip, KeyVal, ScoreRow)
  format.ts        Formatting helpers
```

Screens (in `screens/`, under the "Console" nav group):

- **Dashboard**   mission header, live ten-phase Executive cycle
                 (Observe · Attend · Plan · Simulate · Execute · Verify ·
                 Reflect · Consolidate · Learn · Idle), world state, Runtime
                 card (connected / bridge / latency / updates-per-sec /
                 records), recent decisions
- **Timeline**    full decision reconstruction: request → plan → prediction → outcome → feedback
- **Explanation** reason chips, candidate scoring, experience/reputation/calibration factors
- **Metrics**     the derived `MetricsReport` (accuracy, calibration, fallback, reputation, experience)
- **Ledger**      raw immutable decision records

The provider seam is `ConsoleDataSource`. It is **streaming-capable**: besides
the request/response `get*` methods, both providers expose `connect()` /
`disconnect()` / `subscribe(listener)` and publish typed `ConsoleFrame`s — a
hello, then the full snapshot, then additive deltas (executive phase advance,
ledger append, metrics refresh), heartbeats carrying measured round-trip
latency, and error frames. The controller `switch(frame.kind)` merges frames
into the `useConsole()` snapshot, detects dropped frames (sequence gaps), and
tracks arrival rate + latency for the Runtime card. Replacing the mock with the
runtime bridge never touches a screen — you swap the provider via
`setConsoleSource()`, never the mock.

`RuntimeConsoleDataSource` is wired to the Rust **ReadModelBridge**
(`services/bridge`, `friday-bridge`) over `ws://127.0.0.1:18433`; run
`cargo run -p friday-bridge` from the repo root to serve the console. The
bridge serializes the frozen read models losslessly (serde_json
`float_roundtrip`) into the same `ConsoleFrame` shapes, tagged by `kind` with
`protocol: 1`.

The client answers the bridge's `hello` with a capability handshake
(`{"type":"hello_response","capabilities":{"metrics":true}}`) so it enables the
optional `metrics` counter frames, then receives a `capabilities` ack before
the snapshot. `error` frames carry the machine-readable taxonomy
(`code`/`severity`/`recoverable`/`recommended_action`). The full protocol —
state machine, per-state frame legality, negotiation flow, error codes — is
frozen in `services/bridge/PROTOCOL.md`.

Choose the provider at startup with `VITE_CONSOLE_BRIDGE`:

```bash
npm run dev                                  # mock (default, no bridge needed)
$env:VITE_CONSOLE_BRIDGE = "1"
npm run dev                                  # real bridge @ ws://127.0.0.1:18433
```

Same interface, same frames — zero screen changes. `RuntimeConsoleDataSource`
also **auto-reconnects** with exponential backoff (500 ms → 8 s cap, reset on a
healthy snapshot; no retry after an explicit `disconnect()`), and each hello
opens a new reconnect epoch in the store (sequence tracking resets, cumulative
counters persist).

The Executive panel mirrors the real ten-phase cycle from
`services/goal-execution/src/executive.rs` (frozen Core 1.0), including the
**Investigate** phase, and the Execution Graph renders the live
`InvestigationGraph` emitted by the Executive — driven by the Rust
`DemoRuntimeSource`/bridge when `VITE_CONSOLE_BRIDGE=1` and `cargo run -p
friday-bridge` is serving.

## How to run

```bash
cd frontend
npm install
npm run dev      # Vite dev server (http://localhost:5173)
npm run build    # Production build to dist/
```

Set `VITE_FRIDAY_WS` to point at a running `friday-mobile-daemon`:

```bash
$env:VITE_FRIDAY_WS = "ws://192.168.1.50:18432"
npm run dev
```

## Connecting the portal (live daemon)

The portal's `FridayClient` connects to `VITE_FRIDAY_WS` (default
`ws://localhost:18432`) — the **Mobile Protocol daemon**
(`services/mobile`, `cargo run -p friday-mobile --bin friday-mobile-daemon`).
Nothing else is needed for local dev **if the daemon runs in open mode** —
pairing auth is on by default, so start it with:

```powershell
$env:FRIDAY_MOBILE_AUTH = "off"
cargo run -p friday-mobile --bin friday-mobile-daemon
```

**Or just run the one-command boot script** (from `frontend/`):

```bash
npm run dev:portal                 # daemon + bridge + Vite
npm run dev:portal -- -Port 5175   # same, portal on :5175
npm run dev:portal:keep            # leave the daemon up after Vite exits
```

`scripts/dev-portal.ps1` boots **both** background services: the daemon with
`FRIDAY_MOBILE_AUTH=off` when nothing is listening on `:18432`, and the
ReadModelBridge (`friday-bridge`) when nothing is listening on `:18433` — each
reused if already running. It then starts Vite with `VITE_CONSOLE_BRIDGE=1`
(so the Engineering Console is live, not mocked) and stops the services it
started when Vite exits. `dev:portal:keep` passes `-KeepDaemon` so the daemon
stays up for the next portal session; pass `-KeepBridge` too (or run
`powershell -File scripts/dev-portal.ps1 -KeepDaemon -KeepBridge`) to keep the
bridge. Use `-SkipBridge` to boot daemon + Vite only (console uses the mock).

In open mode the handshake is `Hello → Welcome(requires_auth=false)` and the
client reaches `connected` immediately. The Home screen's **Resources card** is
derived from `telemetry.system` bus events (real CPU/RAM/Disk every 2 s from
the daemon's `SystemProbe`), so a connected portal shows live numbers — no
mock data. (When the daemon is absent, the client reports `disconnected` and
Home renders "FRIDAY is offline." honestly.)

## Status

🟢 **Foundation complete** — design system, component library, all six screens,
protocol client, navigation shell. TypeScript compiles clean, Vite builds clean.

✅ **Wired to the real `friday-mobile-daemon` and verified end-to-end** —
open-mode handshake + live `telemetry.system` frames confirmed on the wire.

Next: Tauri integration and desktop animations polish.

**Phase 1 Engineering Console (Week 1) done** — Dashboard, Timeline,
Explanation, Metrics, Ledger screens consuming mocked read models. Dashboard
now shows the real ten-phase Executive cycle and a full World State. TypeScript
compiles clean, Vite builds clean, oxlint clean.

**Week 2 (done)** — `ConsoleDataSource` provider seam landed with
`MockConsoleDataSource`; the interface is **streaming-capable**
(`connect`/`subscribe`/`disconnect` publishing typed `ConsoleFrame`s) and
the Dashboard gained a **Runtime card** (connected / bridge / latency /
updates-per-sec / records) fed by live ticks. The Rust `ReadModelBridge`
(`services/bridge`) serves the frozen read models over WebSocket at
`ws://127.0.0.1:18433`, and `RuntimeConsoleDataSource` consumes it — verified
end-to-end (hello → snapshot → deltas → heartbeat, contiguous sequences,
140 records). Mock ↔ runtime is one `setConsoleSource()` call.

**Bridge hardening (done)** — typed frame protocol v1 with bridge
telemetry. Hello/heartbeat frames carry `bridge_stats` (connections,
reconnects, resyncs, frames/bytes sent, snapshot/delta wire sizes); the store
validates frame sequences strictly (gaps/out-of-order/dups counted as drops,
reconnect epochs reset per hello); the Runtime card shows Reconnects, Avg/Peak
RTT, snapshot and delta sizes; `RuntimeConsoleDataSource` auto-reconnects.
Verified: bridge unit tests (11), E2E smoke (resync, stats, reconnect
counting), reconnect smoke (bridge down → up), Runtime DS smoke (hello →
snapshot, 140 records). Next: a source switch in the UI, then the Replay
viewer (Week 3) and the memory explorer (Week 4).
