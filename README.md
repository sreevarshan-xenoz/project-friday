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

Qualification state is tracked in [QUALIFICATION-STATUS.md](QUALIFICATION-STATUS.md).
That document is the source of truth for `PASS` / `PARTIAL` / `BLOCKED` /
`DEFERRED` project status.

| Area | State |
| ---- | ----- |
| M0 Foundation (structure, docs, tooling) | ✅ Done |
| M1 Core (event bus, plugin SDK, protocol, runtime) | ✅ Done |
| M2 Memory (working / session / long-term / vector + experience) | ✅ Done |
| M3 Planner (intent → plan → execute → verify) | ✅ Done |
| M4 Agent Kernel (permissions, audit, lifecycle, adapters) | ✅ Done |
| M4.5 Services (filesystem, terminal, git, Windows bridge, web, docs, codeintel, browser adapter) | ✅ Implemented; browser runtime qualification deferred |
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
| Vision / Voice / Learning / Active Inference | Planned (Phase 5 of the synthesis) |
| Autonomous Cognitive Tick | ✅ Live — bounded tested runtime; `LiveRuntimeSource` runs through the real bridge process, executes/verifies a safe kernel action, exposes provenance, and rehydrates the journal without duplicate dispatch |

See [`docs/Roadmap.md`](docs/Roadmap.md), [`docs/FRIDAY-20-ECOSYSTEM-ROADMAP.md`](docs/FRIDAY-20-ECOSYSTEM-ROADMAP.md),
[`AGENTS.md`](AGENTS.md), and [`specs/README.md`](specs/README.md)
for the authoritative milestone, subsystem, and agent operating guidelines.

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
| 6 | **Evidence pipeline: Web + Docs → Verify → Memory** | 🟢 DONE |
| 7 | **Agent Director / bounded subagents** | 🟢 DONE |
| 8 | **Parallel durable workflows + agent journaling** | 🟢 DONE |
| 9 | **Causal World Model** (entities → causes → effects → simulation) | 🟢 DONE |
| 10 | **LifeOS current→ideal/gap layer** (Current → Desired → Gap → Objective → Plan) | 🟢 DONE |
| 11 | **Skill compounding** (verified trajectory → Skill Registry → reuse) | 🟢 DONE |
| 12 | **Egress policy + Secrets vault** (autonomy prerequisites) | 🟢 DONE |
| 13 | Screen perception | 🟢 DONE |
| 14 | Audio perception | 🟢 DONE |
| 15 | Vision perception | 🟢 DONE |
| 16 | Persistent Agent Workforce | 🟢 DONE |
| 17 | Autonomous Cognitive Tick (last) | 🟢 **DONE** |
| 18 | Observability | 🟢 DONE |
| 19 | Cost telemetry | 🟢 DONE |
| 20 | Walrus Memory backend (optional — 5th `MemoryLayer` backend) | optional |
| 21 | World-model visualization / DrawDB | 🟢 DONE |

**Where we are:** **Gate 1 (Information Organ) is complete** — AnyDoc · Web
Search · Web Fetch · Evidence · Memory are all wired — and **#5/#6, the real
local Code/Test Verifier and the Evidence pipeline, are done**: FRIDAY now
interprets observed output instead of trusting it (`CodeTestVerifier` → real
command run → inspectable `checks[]`; a failed check is `Failed`, never
`Verified`), and the two organs blend into one verified research body — AnyDoc
+ Web → unified `Evidence[]` → `EvidenceVerifier` → episodic memory
(`evidence_pipeline.rs`). That closes the core of **Gate 2 (Trustworthy
Executor)** — Planning · Execution · **Verification** · Permissions · Audit ·
Durability are all real. **#7 — the Agent Director — is done**: one parent
spawns bounded, role-typed children (Researcher / Analyst / Verifier) and the
goal completes only when every result passes the trust gate
(`AgentDirector::verify` → `DirectorReport::all_verified`,
`services/agent-runtime/src/director.rs`), wired through the real
`SkillAgentDriver` (`services/action-runtime/tests/agent_director.rs`). That is
the first step into **Gate 3 (Multi-Agent FRIDAY)**. **#8 — parallel durable
workflows + agent journaling — is done**: the `Workflow` trait gained a
**parallel stage** step (`WorkflowStep::stage`) — bounded-concurrency fan-out
(the Director topology made durable), each child journaling + verifying
independently and folded exactly once by `on_stage_completed` — plus
**write-ahead in-flight journaling** with stable FNV-1a idempotency keys
(`JournalEntry`/`journal_key`): every dispatch persists InFlight before it runs
and the terminal outcome after, so a crash resumes with explicit, observable
recovery (`checkpoint.recovered`), verified steps are never re-dispatched, and
a Verified entry + state fold + cursor advance land in one atomic save
(`services/goal-execution/src/workflow.rs`; `with_store` engine-owned
persistence over `WorkflowStore`). The Director's topology is now durable.
**#9 — the causal World Model — is done**: the `WorldModel` now carries a
**causal layer** (`services/goal-execution/src/causal.rs`) — a grounded
**entity graph** (`Entity` id/kind/properties + relations), typed **causal
edges** (action → effects gated by `CausalCondition`), and **pure
simulation** (`CausalModel::simulate`): the first applicable edge fires and its
effects project S' — answering exactly "if action A runs, state S changes to
S'" without touching the real model (counterfactual). `WorldModel::simulate` /
`apply_simulation` close the predict→commit loop with a stale-projection guard,
and `find_missing_capability_prerequisites` now reflects the **entity graph** —
an observed `GitRepository` entity satisfies `git.commit` with no `.git` path.
**#10 is done:** the LifeOS intent layer (`services/goal-execution/src/intent.rs`)
makes current→ideal real — a declarative `DesiredState` is evaluated against
the world state + causal entity graph, the open `Gap` becomes an `Objective`
whose closing actions are chosen by **causal simulation** (an action closes the
gap only when its projection satisfies the ideal, never execution), and
`IntentLayer::plan` feeds the objective into the planner producing
`PlanSource::GapDriven` plans. `IntentLayer::progress` and `is_complete` now
recalculate progress from the authoritative WorldModel after transitions. A
closed gap yields nothing — the layer never invents busywork. INT-01–INT-08
are accepted in `docs/LIFEOS-INTENT-ACCEPTANCE.md`. `IntentDecision` now composes
that existing layer as the sole decision source for the live bridge path; the
acceptance remains bounded rather than generalized.
**#11 is done:** the learning loop is closed. `SkillCompiler`
(`services/skills/src/compound.rs`) turns a **verified successful trajectory**
into a registered skill — the refusal gate IS the verification step (an
`Unverified` outcome is never compounded; below-bar confidence/support, empty
steps, and unrankable actions all refuse with structured reasons). Success
registers a `SkillSource::Learned` skill **disabled as a candidate** with every
used capability resolved and emits `skill.compounded` carrying the evidence
trail; explicit admission (`qualify` → `SkillRegistry::enable_learned`) makes it
selectable, so a learned skill is never live before it is admitted. The runtime
seam `SkillLearningService` (`services/context/src/skill_learning.rs`) refuses
trajectories with no planner-resolvable capability and re-resolves statuses
honestly against the live registry. Proven end-to-end: the goal-execution
integration test verifies a trajectory with the real `CodeTestVerifier`,
compiles it, admits it, and a later goal's `select_for` finds and chooses it;
`services/context/tests/skill_learning.rs` runs the same loop over the real
wiring with a second verified execution.
**#12 is done:** the autonomy prerequisites are real.
`SecretsVault` (`services/intelligence/src/secrets.rs`) holds credentials in
`Secret` values that are **structurally unobservable** — `Debug`/`Display`/
`Serialize` all render a redaction, only `reveal()` exposes a value — with
capability-scoped handles and env seeding. `EgressGate`
(`services/intelligence/src/egress.rs`) answers "can this secret leave the
machine? which provider receives it?": it scans every outbound prompt for
registered secrets and applies a **per-provider** Block / Redact / LogOnly
policy (safe default: Block), wired into `InferenceFabric` at both dispatch
sites — a Block is recorded on the routing decision before the error returns.
Enabling is one line (`fabric.egress = Some(gate)`); it stays off by default.
**#13 is done — the first real sensor is live.** `WindowsScreenSensor`
(`services/sensors/src/screen.rs`) closes the constitution's "perception is
synthetic RNG" gap for the screen: each poll captures the **real foreground
window** — title/pid/process, bounds, state — plus the **continuous screen
diff**: an FNV-1a fingerprint of the client-area pixels via BitBlt+GetDIBits.
Change detection is pure and precedence-correct (identity/geometry → Focus,
same-window pixel change → Content), capture failure degrades honestly to
`content_hash: None`, and the first poll emits the initial observation so
FRIDAY sees its screen state on tick 1. Events flow
`SensorLoop::poll_all_with_topics` → `GlobalWorkspace` (`Topic::Screen`), and
the perception engine's `with_sensors` mode reports exactly the channels that
fired — never a wildcard guess.
**#14 is done — the audio organ is real.** `AudioSensor`
(`services/sensors/src/audio.rs`) runs mic → VAD → ASR → speaker/turn event:
WASAPI shared-mode capture of the default input device (16 kHz mono PCM,
`GetMixFormat` fallback for hardware that rejects it) → a pure energy VAD
(hangover extends speech, never creates it) → a pure turn detector (speech run
closed by silence) → the optional `SpeechToText` seam. Honest by construction:
a turn event carries `transcript: null` / `asr: null` until a real ASR backend
is attached — the pipeline never fabricates words it did not hear — and a
headless machine with no mic produces nothing. Events flow
`SensorLoop::poll_all_with_topics` → `GlobalWorkspace` (`Topic::TurnDetected`),
and the full VAD → turn → event path is tested end-to-end over synthetic PCM.
**#15 is done — the vision organ is real, and the whole perception triad is
complete.** `CameraSensor` (`services/sensors/src/vision.rs`) runs camera →
frame → vision model → object/event: Media Foundation webcam capture
(RGB32 negotiated, dimensions never guessed) → pure frame differencing
(BT.601 luma) → a deterministic connected-component blob detector producing
bounding boxes with confidence → the optional `ObjectDetector` ML seam
(YOLO/ONNX). Honest by construction: the first frame is a baseline and
*one frame sees nothing* (motion is between frames), a static scene emits
nothing, and `detector: "diff"` names the built-in — never a fabricated label.
Events flow `SensorLoop::poll_all_with_topics` → `GlobalWorkspace`
(`Topic::ObjectDetected`). Screen + Audio + Vision are all real now.
The persistent workforce (#16) is real too: `Workforce`
(`services/agent-runtime/src/workforce.rs`) — a persistent worker registry
+ assignable task board over `AgentRuntime` + a `WorkforceStore`; a worker
is assigned a task, picked up across runs (fresh instances over one Json
store), reports progress/blockers, and its isolated workspace (`spec.cwd`)
persists — proven with the real `SkillAgentDriver` across three instances
(`tests/workforce.rs`).
#17 is complete for the tested live-runtime path.
The real runtime path is live in
`LiveRuntimeSource`: it runs the real `AutonomyHost` through the Action Kernel,
performs and verifies safe `git.status`, exposes event/evidence provenance over
WebSocket, and proves fresh-instance journal recovery without duplicate dispatch.
The causal World Model and LifeOS intent layer are implemented and audited in
`docs/WORLD-MODEL-ACCEPTANCE.md`. Runtime feedback is now wired through the
same host: verified Action Kernel observations update the sole `WorldModel`,
publish `world.updated`, and drive a second decision; see
`docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md`. The current qualification target
is broader integration coverage, so Gate 7 remains a bounded tested-runtime
claim rather than generalized autonomy.

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

# Cognitive runtime / autonomous tick
cargo test -p friday-cognitive

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
| [`services/cognitive/README.md`](services/cognitive/README.md) | Cognitive runtime and autonomous tick |
| [`docs/PARALLEL-DURABLE-ACCEPTANCE.md`](docs/PARALLEL-DURABLE-ACCEPTANCE.md) | PD-01–PD-07 durable parallel execution receipt |
| [`docs/WORLD-MODEL-ACCEPTANCE.md`](docs/WORLD-MODEL-ACCEPTANCE.md) | WM-01–WM-09 world-model audit and receipt |
| [`docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md`](docs/RUNTIME-WORLD-FEEDBACK-ACCEPTANCE.md) | Verified runtime outcome → WorldModel → next-tick receipt |
| [`docs/LIFEOS-INTENT-ACCEPTANCE.md`](docs/LIFEOS-INTENT-ACCEPTANCE.md) | INT-01–INT-09 current/desired/gap/planner receipt |
| [`docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md`](docs/CLOSED-COGNITIVE-LOOP-ACCEPTANCE.md) | CC-01–CC-12 whole-loop regression receipt |
| [`docs/FRIDAY-INTEGRATION-TEST-PLAN.md`](docs/FRIDAY-INTEGRATION-TEST-PLAN.md) | Repeatable integration campaigns and baseline status |
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
