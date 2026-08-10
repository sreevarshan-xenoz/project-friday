# PROJECT FRIDAY

> **Mission:** Build the world's best open-source desktop AI agent that can use
> Windows like a human.
>
> Not "an AI assistant." A genuine **Desktop Intelligence Platform** — the
> operating system's AI layer.

FRIDAY follows one loop:

> **Understand. Plan. Execute. Verify. Learn.**

The core contains **no intelligence of its own**. All behaviour lives in
swappable **plugins / services** that communicate only through an **event bus**
and a **protocol transport**. This makes the system modular, testable, and able
to grow from a simple desktop assistant into an autonomous operating companion
without a rewrite.

## Status

**Phase: Alpha Qualification + FRIDAY 2.0 synthesis** — the M0–M5 architecture
is complete and the runtime API is frozen. Work now builds the layered cognitive
stack on top of the proven core (evidence lifecycle, durable workflows,
investigation, agent runtime, memory fabric) while qualification keeps proving
reliability with benchmarks.

| Area | State |
| ---- | ----- |
| M0 Foundation (structure, docs, tooling) | ✅ Done |
| M1 Core (event bus, plugin SDK, protocol, runtime) | ✅ Done |
| M2 Memory (working / session / long-term / vector + experience) | ✅ Done |
| M3 Planner (intent → plan → execute → verify) | ✅ Done |
| M4 Agent Kernel (permissions, audit, lifecycle, adapters) | ✅ Done |
| M4.5 Services (filesystem, terminal, git, Windows bridge, web, docs, codeintel, **browser**) | ✅ Done |
| M4.6 Capability Intelligence + Service Orchestrator | ✅ Done |
| M4.7 Plan Simulation | ✅ Done |
| M4.8 Experience Engine (procedural memory) | ✅ Done |
| M4.10 Context Builder | ✅ Done |
| M5 Skill Engine (installable capability bundles) | ✅ Done (`services/skills`) |
| M5 Cognitive / Intelligence layer (capability routing, qualification profiles, health telemetry) | ✅ Done |
| MCP as first-class `ActionAdapter` | ✅ Done |
| Mobile daemon + Android console client + web portal | ✅ Live (auth knob + real telemetry) |
| Benchmark + Recovery + Windows / Provider qual harnesses | ✅ In use (chaos lab: 18 scenarios) |
| Planner is skill-aware (RELEVANT SKILLS + EVIDENCE REQUIRED prompt) | ✅ Done |
| Investigation Engine (heuristic + LLM strategies, evidence-gated plans) | ✅ Done (`services/investigation`) |
| Agent Runtime (sub-agent orchestration, scoped contexts) | ✅ Done (`services/agent-runtime`) |
| Evidence lifecycle (`Prepared → Observed → Verified`) | ✅ Done (goal-execution) |
| Durable execution (workflow trait + checkpoint/resume) | ✅ Done (goal-execution) |
| Memory fabric (admission controller, Knowledge Graph, World Model) | ✅ Done |
| Persona Engine (identity + blended personas + PAD mood) | ✅ Built (`services/persona`), not yet wired |
| Persistent Project Cognition (long-term goal memory) | ✅ Done (`services/project-cognition`) |
| Skill Forge (create / validate / publish CLI) | ✅ Done (`services/skill-forge`) |
| Desktop UI (React + TypeScript + engineering console) | ✅ Live (Execution Graph, bridge-driven) |
| Vision / Voice / Learning / Autonomy / Active Inference | Planned (Phase 5 of the synthesis) |

See [`docs/Roadmap.md`](docs/Roadmap.md) and [`specs/README.md`](specs/README.md)
for the authoritative milestone and subsystem map.

## Master Plan — the corrected build order

The operational build order, **tracked here** and in
[`docs/MasterPlan.md`](docs/MasterPlan.md) (definitions of done + rationale + update
log). It re-sequences the constitution's P0–P5 tiers into the order we actually
build; the audit trail and `file:line` evidence live in
[`specs/architecture/friday-architecture-vnext.md`](specs/architecture/friday-architecture-vnext.md).
Statuses are updated as commits land — this table is the tracker.

| # | Work | Status |
| - | ---- | ------ |
| 1 | AnyDoc `docs.parse` | 🟢 DONE |
| 2 | Agent-Reach resolver (binding resolver + health-gated fallback) | 🟢 DONE |
| 3 | `web.search` resolver wiring (normalized `Evidence[]`) | 🟢 DONE |
| 4 | `web.fetch` resolver wiring (normalized fetch `Evidence[]`) | 🟢 DONE |
| 5 | **Real local Code/Test Verifier** (`ConcreteVerifier`, before Sonar) | 🟢 DONE |
| 6 | **Evidence pipeline: Web + Docs → Verify → Memory** | 🟡 **NEXT** |
| 7 | **Agent Director / bounded subagents** | 🔴 |
| 8 | **Parallel durable workflows + agent journaling** | 🟡 |
| 9 | **Causal World Model** (entities → causes → effects → simulation) | 🟡 |
| 10 | **LifeOS current→ideal/gap layer** | 🔴 |
| 11 | **Skill compounding** (only after real verification) | 🟡 |
| 12 | **Egress policy + Secrets vault** (before autonomy) | 🔴 |
| 13 | Screen perception | 🔴 |
| 14 | Audio perception | 🔴 |
| 15 | Vision perception | 🔴 |
| 16 | Persistent Agent Workforce | 🔴 |
| 17 | Autonomous Cognitive Tick (last) | 🔴 |
| 18 | Observability | 🔴 |
| 19 | Cost telemetry | 🔴 |
| 20 | Walrus Memory backend (optional — 5th `MemoryLayer` backend) | optional |
| 21 | World-model visualization / DrawDB | last |

**Where we are:** **Gate 1 (Information Organ) is complete** — AnyDoc · Web
Search · Web Fetch · Evidence · Memory are all wired — and **#5, the real local
Code/Test Verifier, is done**: FRIDAY now interprets observed output instead of
trusting it (`CodeTestVerifier` → real command run → inspectable `checks[]` →
`VerifiedResult`; a failed check is `Failed`, never `Verified`). That is the
first concrete step into **Gate 2 (Trustworthy Executor)**. Next unit of work:
**#6 — the Evidence pipeline** (Web + Docs → Verify → Memory).

### Milestone gates

| Gate | Name | Contents |
| ---- | ---- | -------- |
| 🟢 1 | Information Organ | AnyDoc · Web Search · Web Fetch · Evidence · Memory — ✅ complete |
| 🟢 2 | Trustworthy Executor | Planning · Execution · Verification · Permissions · Audit · Durability — *next serious milestone* |
| 🟢 3 | Multi-Agent FRIDAY | Agent Director · Subagents · Parallel workflows · Recovery |
| 🟢 4 | World-Aware FRIDAY | World Model · Intent · Gap Analysis · Causal Planning |
| 🟢 5 | Learning FRIDAY | Reflection · Verification · Skill Compounding · Persistent Procedures |
| 🟢 6 | Perceptive FRIDAY | Screen · Audio · Vision |
| 🟢 7 | Autonomous FRIDAY | Observe → Reason → Decide → Act → Verify → Learn → Repeat |

## Repository layout

```
FRIDAY/
├── docs/            Design docs (Vision, Architecture, Modules, Security, …)
├── specs/           Source-of-truth subsystem specs + ADRs (prefer for new work)
├── engineering/     Process: qualification, certification, benchmarks, release
├── architecture/    Diagrams / ADR index
├── core/            friday-core — event bus, plugins, memory, protocol, runtime
├── planner/         friday-planner — four reasoning engines
├── services/        Long-lived capability services (kernel, caps, sim, intel, skills,
│                   investigation, agent-runtime, persona, knowledge, goal-execution, …)
├── mcp/             friday-mcp — MCP transport, adapter, discovery
├── plugins/         Capability plugins (e.g. windows bridge client)
├── windows-host/    C# Windows host (Win32 / UIA / COM / PowerShell)
├── clients/         External clients (Android console)
├── e2e/             End-to-end test harness (incl. live local-model + skill suites)
├── benchmarks/      Qualification run evidence (Windows, provider, …)
├── artifacts/       Reproducible build outputs (e.g. Android APK)
├── frontend/        React + TypeScript desktop UI (design system + components + screens + console)
├── skills/          Bundled skill manifests (git, docker, web, python, …)
├── desktop/         Legacy path note — live host is windows-host/
├── memory/          Memory layer notes (impl lives in core/ + services)
├── models/          Model registry notes (impl in services/intelligence)
├── sdk/             Cross-language SDK / protocol schema (in progress)
├── tests/           Cross-crate integration notes
├── scripts/         Build / dev automation
├── validation/      Unattended long-running validation rig (VP-001)
├── assets/          Icons, sounds, branding
└── prototypes/      Throwaway spikes
```

## Quick start (Core)

Requires Rust 1.85+.

```powershell
cargo build -p friday-core
cargo test  -p friday-core
cargo run   -p friday-core --example echo_plugin
```

The `echo_plugin` example registers a plugin that answers `demo.ping` with
`demo.pong`, demonstrating the full event-bus pipeline.

### Useful workspace targets

```powershell
# Planner + kernel pipeline tests
cargo test -p friday-planner
cargo test -p friday-actions

# Skill Engine / Goal Execution (evidence lifecycle, workflows, admission)
cargo test -p friday-skills
cargo test -p friday-goal-execution

# Investigation / Agent Runtime / Persona / Knowledge / Project Cognition
cargo test -p friday-investigation
cargo test -p friday-agent-runtime
cargo test -p friday-persona
cargo test -p friday-knowledge
cargo test -p friday-project-cognition

# Capability intelligence / simulation / recovery
cargo test -p friday-capabilities
cargo test -p friday-simulation
cargo test -p friday-recovery

# Provider qualification harness + chaos laboratory (18 scenarios)
cargo run -p provider-qual -- --help
cargo run -p provider-qual -- --chaos --mock

# Benchmark harness (optional CHAOS mode)
cargo run -p friday-benchmark -- 600
# $env:CHAOS=1; cargo run -p friday-benchmark -- 600

# Windows qualification (mock-friendly; real host opt-in)
cargo run -p windows-qual -- --help
```

Windows host (C#, .NET 8):

```powershell
dotnet build windows-host/windows-host.csproj -c Release
```

Android console: see [`clients/android/README.md`](clients/android/README.md).

Mobile daemon (speaks the Mobile Protocol on `0.0.0.0:18432`; the web portal
and Android client both talk to it):

```powershell
# Pairing on by default for phones; disable for local dev / the web portal
$env:FRIDAY_MOBILE_AUTH = "off"
cargo run -p friday-mobile --bin friday-mobile-daemon
```

Frontend dev server (portal):

```powershell
cd frontend
npm install
npm run dev     # http://localhost:5173 (default Vite port)
```

## Documentation

| Start here | Why |
| ---------- | --- |
| [`docs/Vision.md`](docs/Vision.md) | What FRIDAY is / is not |
| [`docs/Architecture.md`](docs/Architecture.md) | Event-driven design + subsystem addendum |
| [`docs/Roadmap.md`](docs/Roadmap.md) | Milestone history and current phase |
| [`specs/README.md`](specs/README.md) | **Source of truth** for subsystem contracts |
| [`docs/PluginSDK.md`](docs/PluginSDK.md) | Plugin contract |
| [`engineering/qualification.md`](engineering/qualification.md) | How we measure readiness |

### Locked architecture documents

These define the frozen foundation and require explicit review + a version bump
to change:

`Architecture.md`, `Planner.md`, `Execution.md`, `Memory.md`, `Security.md`,
`PluginSDK.md`, `API.md`, `docs/research/` (original research).

Event vocabulary and `ActionAdapter` / kernel contracts are also locked under
`specs/`.

Every new feature must answer: **which event does it consume? which event does
it produce? which plugin/service owns it?**

## Contributing

- Branch from `dev` into `feature/*`; never commit to `main`.
- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Prefer a **benchmark or measurable reliability gain** over feature-only PRs
  during Alpha Qualification (see `docs/Roadmap.md`).
- Every module is documented **before** its code when introducing a new subsystem
  (`specs/` first).

Full rules in [`docs/CodingStandards.md`](docs/CodingStandards.md) and
[`engineering/coding_standard.md`](engineering/coding_standard.md).

## License

MIT OR Apache-2.0.
