# services/

Long-lived, addressable capability **services** hosted by the FRIDAY platform.
A service exposes a stable identity, lifecycle, and capabilities; the planner and
kernel reason over those capabilities rather than hardcoding plugin names.

The low-level `Service` trait remains in `core/src/services/traits.rs`. Concrete
crates live here.

## Workspace crates

| Crate | Path | Role | Status |
| ----- | ---- | ---- | ------ |
| `friday-actions` | `action-runtime/` | **AgentKernel** — permissions, audit, lifecycle, sessions, resources, metrics, `ActionAdapter` dispatch | ✅ Stable |
| `friday-capabilities` | `capabilities/` | Capability registry, ranking, health, graph, service orchestrator, cost estimator | ✅ |
| `friday-context` | `context/` | Context Builder — assembles `AgentContext` (skill-aware) for the intelligence layer | ✅ |
| `friday-skills` | `skills/` | **Skill Engine (M5)** — `SkillLoader` / `SkillRegistry` / `SkillContext`; manifests are pure data; health, approvals, explainable ranking | ✅ |
| `friday-persona` | `persona/` | **Persona Engine** — one identity, blended personas, PAD mood from OCC appraisals, cooldowns, deterministic `render_instruction` | ✅ Built (not yet wired into the runtime) |
| `friday-investigation` | `investigation/` | **Investigation Engine** — strategy layer between goal and plan; heuristic + LLM strategies, evidence requirements, `InvestigationGraph` | ✅ |
| `friday-agent-runtime` | `agent-runtime/` | **Agent Runtime** — sub-agent orchestrator: spawner, lifecycle, mailbox, contexts, budgets, result merge, fan-out decomposition | ✅ |
| `friday-goal-execution` | `goal-execution/` | **Executive** — the cognitive cycle (Observe→Attend→Investigate→Plan→Simulate→Execute→Verify→…); evidence lifecycle (`Prepared→Observed→Verified`), workflow trait + checkpoint/resume, memory admission controller, cognitive trace | ✅ |
| `friday-knowledge` | `knowledge/` | **Knowledge Graph + World Model** — entities, relationships, state; world model wiring for planning | ✅ |
| `friday-project-cognition` | `project-cognition/` | **Persistent project cognition** — long-term goal memory, milestone tracking, decision journal | ✅ |
| `friday-simulation` | `simulation/` | Plan simulation / dry-run / validation / optimization / approval | ✅ |
| `friday-intelligence` | `intelligence/` | **Inference Fabric** — provider registry (llama.cpp, Ollama, OmniRoute, Gemini, OpenGateway, …), capability-based routing, qualification profiles, health telemetry, request timeouts, prompt compiler, response interpreter | ✅ |
| `friday-recovery` | `recovery/` | Recovery graph, failure taxonomy, confidence model | ✅ (shadow mode in runtime) |
| `friday-benchmark` | `benchmark/` | Resilience dashboard, MTBF / first-pass / recovery metrics | ✅ |
| `friday-experience` | `experience/` | Experience engine (procedural memory: recorder, miner, reflection, suggestion, replay) | ✅ |
| `friday-plugin-runtime` | `plugin-runtime/` | Plugin runtime contract, telemetry envelopes, correlation ids | ✅ |
| `friday-sensors` | `sensors/` | Sensor surface for the cognitive OS | 🟡 Early |
| `friday-cognitive` | `cognitive/` | Cognitive Runtime scaffold (`specs/cognitive-runtime.md`) | 🟡 Early |
| `friday-reflection` | `reflection/` | Reflection types (`specs/reflection-engine.md`) | 🟡 Early |
| `friday-evaluation` | `evaluation/` | Mission scorecard / evaluation framework | 🟡 Early |
| `friday-engines-perception` | `engines/perception/` | Perception engine | 🟡 Early |
| `friday-engines-memory` | `engines/memory/` | Memory engine | 🟡 Early |
| `friday-mobile` | `mobile/` | Mobile protocol daemon (WebSocket, `:18432`) for phones + web portal; QR-pairing auth (env-disablable for local dev) | ✅ Live |
| `friday-bridge` | `bridge/` | **ReadModelBridge** — frozen read models over WebSocket (`:18433`) for the engineering console | ✅ |
| `provider-qual` | `provider-qual/` | LLM/provider qualification harness + **chaos laboratory** (18 scenarios: provider failures + health corruption) | ✅ |
| `windows-qual` | `windows-qual/` | Windows action qualification harness | ✅ |
| `friday-skill-forge` | `skill-forge/` | **Skill Forge (M7)** — `create-skill` / `validate` / `publish` CLI for third-party skills | ✅ |

## Adapters (under `action-runtime`)

| Adapter | Tool key | Notes |
| ------- | -------- | ----- |
| Filesystem | `filesystem` | create/read/write/delete/rename/copy/move/list |
| Terminal | `terminal` | Persistent shell session |
| Git | `git` | CLI-backed capabilities (clone, status, commit, …) |
| Web | `web` | HTTP fetch + search (evidence adapter for investigation plans) |
| Docs | `docs` | Local documentation search |
| Code Intelligence | `codeintel` | Repository understanding (symbols, structure) |
| Browser | `web.*` | Real headless Chrome/Chromium via CDP (`chromiumoxide`): open/click/type/scroll/extract/screenshot, persistent sessions (M7) |
| WebAgent | `webagent` | Ask/send/read against **logged-in sites** (ChatGPTAdapter / GrokAdapter / GeminiAdapter / GenericWebAgent, dedicated profile); backs the `external` investigation channel — `gather evidence: external (webagent.ask)` | ✅ |
| Reasoning | `reasoning` | **Transport-agnostic external intelligence**: `ReasoningProvider` (Api/Browser/Local interchangeable) + `ExpertRouter` — FRIDAY picks the best-suited expert by capability profile, `reasoning.consult`; browser transport ships, API/local are intelligence-fabric seams | ✅ |
| MCP | `mcp` / dynamic | First-class `McpAdapter` (see `mcp/` crate + ADR-0007) |
| Simulation | `sim` | Side-effect-free simulation path |

Windows automation is **not** an adapter in this folder alone: Rust client is
`plugins/windows` (`friday-windows`); C# host is `windows-host/`.

## Related

- Specs: `specs/kernel.md`, `specs/capabilities.md`, `specs/services.md`,
  `specs/simulation.md`, `specs/context.md`, `specs/intelligence.md`,
  `specs/recovery.md`, `specs/benchmark.md`, `specs/skill-engine.md`,
  `specs/investigation-engine.md`, `specs/agent-runtime.md`,
  `specs/persona-engine.md`, `specs/project-cognition.md`,
  `specs/friday-2-0-synthesis.md`
- Design notes: `docs/CapabilityIntelligence.md`, `docs/Execution.md`,
  `docs/WindowsBridge.md`
