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
a mock data source stands in for the runtime until the real bridge lands.

```
src/console/
  types.ts         Shape-accurate TS mirrors of the frozen Rust contracts
                   (contract.rs + experience/src/lib.rs)
  mock.ts          Deterministic synthetic ledger + derived read models
                   (LCG-seeded, mirrors VP-004 + the real engines)
  store.ts         Reactive store (getState/subscribe) + useConsole()
  ui.tsx / ui.css  Console primitives (Panel, Stat, Bar, Chip, ScoreRow)
  format.ts        Formatting helpers
```

Screens (in `screens/`, under the "Console" nav group):

- **Dashboard**   mission header, pipeline stage timings, world state, recent decisions
- **Timeline**    full decision reconstruction: request → plan → prediction → outcome → feedback
- **Explanation** reason chips, candidate scoring, experience/reputation/calibration factors
- **Metrics**     the derived `MetricsReport` (accuracy, calibration, fallback, reputation, experience)
- **Ledger**      raw immutable decision records

Replacing `mock.ts` with a bridge (WebSocket/Tauri) later does not touch the
screens — they only subscribe through `useConsole()`.

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

## Status

🟢 **Foundation complete** — design system, component library, all six screens,
protocol client, navigation shell. TypeScript compiles clean, Vite builds clean.

Next: Tauri integration, desktop animations polish, and wiring to the real
`friday-mobile-daemon` for end-to-end testing.

**Phase 1 Engineering Console (Week 1) done** — Dashboard, Timeline,
Explanation, Metrics, Ledger screens consuming mocked read models. TypeScript
compiles clean, Vite builds clean, oxlint clean. Next: connect the console to
the real runtime (Week 2), then the Replay viewer (Week 3), then the memory
explorer (Week 4).
