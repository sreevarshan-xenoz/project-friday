# TRUE FRIDAY Capability Audit — what is actually missing for a daily laptop assistant

> Read-only audit, 2026-08-14. Scope: **Part 1 (TRUE FRIDAY)** only. Server
> FRIDAY, deployment hardening, and the 24 h endurance qualification are parked
> in **Part 2** and out of scope here.
>
> Method: five parallel evidence walks over THINKING / MEMORY / ACTION /
> SUPERVISOR+loops / INTERACTION, each with EXISTS / STUB / MISSING verdicts and
> `file:line` citations. `todo!`/`unimplemented!`/`placeholder`/`seam` greps run
> across every area.

## Headline finding

**Every real engine exists and is individually tested. Almost none of them are
wired to a running product.**

The codebase is a *library of proven machinery* (adapter catalog, kernel,
director, durable workflows, world model, skills, sensors, memory tiers, portal,
android client) plus a small set of **test-only wirings**. The few production
binaries either bypass the supervisor stack (`friday-mobile-daemon` runs the
in-memory `Executive` directly) or are read-only/demo by construction (bridge,
`e2e` console). Nothing continuously runs the autonomous loop. Nothing recalls
memories into prompts. The Windows desktop adapter, the durable workflow
engine, the autonomous tick, and the human approval surfaces are all built —
and none of them are on the power line.

This is why TRUE FRIDAY does not yet *feel* like the assistant in the vision:
not missing engines, but a **missing product harness that turns the engines
into a daily driver** — plus a handful of genuinely absent capabilities (voice
out, desktop element access, real reasoning).

---

## Capability map

```
                  TRUE FRIDAY
                  ┌──────────────────────────────┐
   THINKING       planner (rule+LLM) · intent ·  │  world model · causal sim · reflection
   MEMORY         episodic · semantic · skills · │  experience · knowledge graph
   ACTION         fs · git · terminal · web ·    │  browser (CDP) · docs · codeintel · MCP
                  desktop/WIN32 (unwired)        │
   SUPERVISOR     director · workforce · durable │  workflows · verification · admission
   LOOP           autonomous tick (one-shot) ·   │  learning (write-only)
   INTERACTION    portal (dead by protocol) ·    │  android · bridge (read-only) · CLI ✗
   VOICE          mic+ASR seam · TTS ✗           │
                  └──────────────────────────────┘
```

## THINKING

| Capability | Verdict | Evidence |
| ---------- | ------- | ------- |
| Rule planner + LLM planner (single-shot) | EXISTS | `planner/src/planning/planners/rule.rs`, `llm.rs:321-345` |
| Intent classification | STUB — 4 keyword intents; "LLM classification plugs in later" | `planner/src/intent/classifier.rs:1-14` |
| Post-failure reflection / repair | STUB — `Risk::Safe` placeholder; Repair == Retry | `planner/src/execution/reflection.rs:43,89,27-29` |
| Causal world model + knowledge graph | EXISTS | `services/goal-execution/src/world.rs:363-420`, `causal.rs` |
| Plan-level simulation | STUB — keyword→effect table; causal effects never applied | `services/goal-execution/src/simulation.rs:61-169` |
| LifeOS desired-state / gap | EXISTS (honest; open gaps reported) | `services/goal-execution/src/intent.rs:44-311` |
| LifeOS ↔ planner bridge | MISSING — two disjoint intent systems, no cross-wiring | `intent.rs` vs `planner/src/intent/classifier.rs` |
| Iterative reasoning / self-critique | **MISSING** — zero scratchpad/CoT/self-critique constructs; planner = 1 model call, no revision | grep, `llm.rs:321-345` |
| Reflection on *real* knowledge | STUB — executive reflects on hardcoded `dummy_knowledge` | `services/goal-execution/src/executive.rs:619-628` |
| Insight persistence | MISSING — insights live in an in-memory `Vec` | `services/reflection/src/lib.rs:79,208-225` |

## MEMORY

| Capability | Verdict | Evidence |
| ---------- | ------- | ------- |
| MemoryLayer + redb Session/LongTerm tiers | EXISTS | `core/src/memory/backends.rs:30-116`, `traits.rs:147-156` |
| Semantic search | STUB — lexical `contains()`; Vector layer inert, no embedding provider | `backends.rs:104-108,242-318` |
| Episode admission (5 gates) | EXISTS | `services/goal-execution/src/admission.rs` |
| "Durable" episodic store | STUB — `Vec` + prune, **no persistence**, lost on restart | `services/goal-execution/src/memory.rs:583-614` |
| SemanticMemory → Policy | STUB — promoted policies never persisted/consumed | `services/goal-execution/src/experience.rs:160-402`, `executive.rs:716-723` |
| Planner experience timeline (redb) | STUB — READ wired, **WRITE path test-only**; runtime timeline always empty | `planner/src/experience/timeline.rs`, `planner/src/lib.rs:199-216` |
| Knowledge graph store (atomic JSON) | STUB — persistence only in own unit test; production graph = seed ontology | `services/knowledge/src/graph.rs:365-568`, `world.rs:417` |
| Skill registry / compiler / learning | EXISTS | `services/skills/src/`, `services/context/src/skill_learning.rs` |
| Learned-skill admission in production | STUB — `admit()`/`enable_learned` called only in tests; candidates stay disabled | `services/context/src/skill_learning_observer.rs:14-24` |
| **Recall into prompts at runtime** | **MISSING in production** — `ContextBuilder`→`PromptCompiler` used only in e2e; mobile daemon + bridge never build an AgentContext | `services/context/src/lib.rs:124-163`, `services/intelligence/src/compiler.rs`, `services/mobile/src/goal_runner.rs:522` |

## ACTION

| Capability | Verdict | Evidence |
| ---------- | ------- | ------- |
| fs / git / terminal / web / docs / codeintel / MCP / webagent adapters | EXISTS | `services/action-runtime/src/adapters/*` |
| Browser (CDP) + redirect egress boundary (#12-E) | EXISTS | `services/action-runtime/src/adapters/browser.rs` |
| Permission engine (deny-by-default, hard deny-list) | EXISTS | `services/action-runtime/src/permission.rs:61-112` |
| Windows desktop adapter (real C# host: Win32 click/type/scroll/clipboard/window mgmt) | EXISTS but **UNWIRED** — never registered on any production kernel path; standalone qual harness only | `plugins/windows/src/`, `windows-host/`, `services/windows-qual/` |
| UIAutomation element finder | STUB — always returns `"UIElementMissing"` | `windows-host/UIAutomation/ElementFinder.cs:13-14` |
| COM/Shell | STUB — returns `"Unsupported"` | `windows-host/COM/Shell.cs:9` |
| Sensors (screen/focus/camera/audio) | EXISTS (real capture) but constructed **only in tests** | `services/sensors/src/*` |
| ASR | STUB — `SpeechToText` seam, only `MockTranscriber` | `services/sensors/src/audio.rs:218-249` |
| TTS (FRIDAY speaks back) | **MISSING** — nothing anywhere | grep `tts|synthesizer|speak|audio_out` |

## SUPERVISOR + loops

| Capability | Verdict | Evidence |
| ---------- | ------- | ------- |
| AgentDirector (roles, bounds, gates, handles, budget, cancel) | EXISTS | `services/agent-runtime/src/director.rs`, `runtime.rs`, `handle.rs`, `budget.rs` |
| Workforce (persistent registry + task board) | EXISTS | `services/agent-runtime/src/workforce.rs` |
| Durable workflows PD-01..07 | EXISTS (15+ tests) but **never used in production**; daemons run in-memory `Executive` | `services/goal-execution/src/workflow.rs` |
| Verification (CodeTestVerifier + Evidence + confidence + causal gates) | EXISTS and **wired** (mobile daemon, bridge live) | `services/goal-execution/src/verifiers.rs`, `evidence.rs` |
| Autonomous tick / AutonomyHost | EXISTS, but **one tick per process** in bridge; no daemon runs the loop | `services/goal-execution/src/autonomy.rs`, `services/bridge/src/source.rs:954-982` |
| Learning loop (reflection→policy→skill) | STUB — `learning.learn()` never called at runtime; completion-sink only in tests/MCP | `services/goal-execution/src/learning.rs`, `executive.rs:597-605` |
| Human supervision (approve/pause/cancel) | STUB — `task.approve/pause/cancel` have **zero subscribers**; permission gate auto-approves `AllowOnce`; held episodes never approved | `services/mobile/src/commands.rs`, `services/action-runtime/src/permission.rs:9-13` |
| Cost/observability | PARTIAL — investigation wired; CostLedger no consumer; audit ring in-memory; evaluation types-only | `services/investigation/`, `services/experience/src/cost.rs` |

## INTERACTION + voice

| Capability | Verdict | Evidence |
| ---------- | ------- | ------- |
| Portal chat UI (event-driven goal cards) | EXISTS | `frontend/src/screens/ChatScreen.tsx`, `protocol/client.ts` |
| Portal ↔ runtime loop | **DEAD by protocol** — bridge `Control` accepts only `hello_response`/`resync`; anything else `Ignore`; "FRIDAY is working…" never resolves | `services/bridge/src/connection.rs:401-419` |
| Android client (pairing, tasks, terminal, notifications) | EXISTS | `clients/android/` |
| Android voice input | MISSING — no `RECORD_AUDIO` permission | `clients/android/.../AndroidManifest.xml` |
| Interactive CLI / REPL | **MISSING** | grep `readline|rustyline|dialoguer|interactive` |
| Local models (Ollama always; llama.cpp env-gated) | EXISTS | `services/intelligence/src/providers.rs:1337-1396` |
| Voice end-to-end | **MISSING** — no STT→daemon wiring, no TTS, no wake word | — |

## The actual holes (not Master Plan numbers)

Ranked by what makes FRIDAY *feel* like the daily assistant:

### P0 — The harness (turn engines into a product)
1. **A real interactive host.** One binary that owns the loop, the approval
   surface, the memory write path, and the client protocols. Today the closest
   thing (`friday-mobile-daemon`) bypasses the supervisor stack; the bridge is
   read-only. Candidates: extend the mobile daemon's `GoalRunner` to also drive
   `WorkflowEngine` + `AutonomyHost` + completion sinks, or a new
   `friday-daemon` that wires the whole stack.
2. **Wire the memory path.** Production writer for the durable tiers (episodic,
   timeline, knowledge graph) **and** recall into prompts (`ContextBuilder` →
   `PromptCompiler`) in the real host. Today it is e2e-only.
3. **Make portal chat live.** Add a real `goal`/`approve`/`cancel` path to the
   bridge protocol so the existing chat UI actually talks to a running FRIDAY
   (or point the portal at the daemon's protocol).

### P1 — The missing capabilities that define a daily assistant
4. **Windows desktop on the kernel.** Register `WindowsAdapter` with the real
   Action Kernel so FRIDAY can click/type/scroll/control apps in a goal, with
   the existing permission + audit + egress machinery in front of it. The C#
   host is already built.
5. **Voice: TTS (out) and real ASR (in).** Nothing speaks back today. Attach a
   real STT behind the `SpeechToText` seam and add a TTS adapter + audio output
   so the portal/android can talk to FRIDAY hands-free.
6. **Human approve/steer loop.** Wire the permission gate to a real prompt
   (portal/android) instead of auto-approve; subscribe `approve/pause/cancel`
   to the host so a human can actually supervise.

### P2 — Depth that compounds over time
7. **Real reasoning.** Iterative plan/verify/critique loop behind the LLM
   planner (replacing the single `complete` call), model-backed intent
   classification, and repair that isn't just retry.
8. **Close the learning loop.** Feed real knowledge into reflection
   (`dummy_knowledge` → knowledge graph), persist insights, admit learned
   skills in production, and let `SemanticMemory` policies actually affect
   behavior.
9. **Simulation honesty.** Make `DefaultSimulator` apply `CausalSimulation`
   effects instead of the keyword table.

## Non-goals (Part 2, parked)

Server daemon, server filesystem layout, 24/7 operation, resource tuning,
restart qualification, 24 h endurance, home-server networking, remote FRIDAY.
Do not touch the 24 h test until the harness above exists and the laptop
version is the daily driver.
