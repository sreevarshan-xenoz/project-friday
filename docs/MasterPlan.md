# Master Plan — the corrected build order (tracker)

> This is the **operational tracker** for FRIDAY's build order. The audit trail
> (what exists, with `file:line` evidence, and the P0–P5 engineering tiers)
> lives in
> [`specs/architecture/friday-architecture-vnext.md`](../specs/architecture/friday-architecture-vnext.md).
> This doc re-sequences those tiers into the order we actually build and records
> **why**. The same table (condensed) is mirrored in the repo
> [README](../README.md#master-plan--the-corrected-build-order).
>
> **Update this table every time a work item lands** — see
> [Update discipline](#update-discipline).

**Status legend:** 🟢 DONE (code + tests exist and are wired) · 🟡 partial /
framework exists but key paths not wired · 🔴 not started · **NEXT** = current
work.

## Master order

| # | Work | Status | Definition of done |
| - | ---- | ------ | ------------------ |
| 1 | AnyDoc `docs.parse` | 🟢 DONE | Docs adapter parses real documents; constitution P0 row DONE (2026-08-10) |
| 2 | Agent-Reach resolver | 🟢 DONE | `BindingResolver` with health-gated fallback + structured `ResolvedOutcome` — `c337e7e`, `820ec5d` |
| 3 | `web.search` resolver wiring | 🟢 DONE | `ResolvingWebTool`: resolver → Action Runtime → normalized `Evidence[]`; healthy + fallback acceptance tests; constitution 🟢 — `318127a` |
| 4 | `web.fetch` resolver wiring | 🟢 DONE | Resolver-backed fetch tool (`ResolvingWebTool::for_web`/`for_web_fetch`) with fallback tests + normalized fetch evidence; Web resolver row fully 🟢 in the constitution |
| 5 | Real local Code/Test Verifier | 🟢 DONE | `CodeTestVerifier`: workspace + command → real execution → stdout/stderr/exit code → `checks[]` → `VerifiedResult` (`services/goal-execution/src/verifiers.rs`). First concrete verifier, **not** Sonar — plus `OutputContains`/`OutputExcludes`/`ToolSuccess` sub-verifiers; 10 tests green |
| 6 | Evidence pipeline hardening | 🟢 DONE | AnyDoc + Web → unified `Evidence[]` → Verification → Memory. `EvidenceSet` blends both organs (dedupe + cross-channel corroboration), `EvidenceVerifier` gates the body behind the frozen `Verifier` seam, the executive publishes the verified body and consolidates it through the admission gate (`services/goal-execution/src/evidence_pipeline.rs`) |
| 7 | Agent Director / bounded subagents | 🟢 DONE | One parent safely creates bounded children and collects **verified** results. `AgentDirector` over `AgentRuntime`: role-typed children (Researcher/Analyst/Verifier), `DirectorConfig` bounds (pool/deadline/steps), `verify()` gates every result (`default_gate`: state + summary + verifier-ran-a-step) — goal completes only when `DirectorReport::all_verified`. Wired through the real `SkillAgentDriver` (acceptance: `services/action-runtime/tests/agent_director.rs`). No persistent workforce yet (that's #16) |
| 8 | Parallel durable workflows + agent journaling | 🟡 **NEXT** | Enhancement on existing `WorkflowCheckpoint`/`WorkflowStore`/`resume()`: parallel stages + agent journaling + idempotency + mid-agent recovery. After #7 — know the execution topology before making it durable |
| 9 | Causal World Model | 🟡 | `WorldModel` gains causal edges + action effects + simulation (Entity → State → Relation → Cause → Action → Effect → Simulation). Not DrawDB |
| 10 | LifeOS current→ideal/gap layer | 🔴 | Current State → Desired State → Gap → Objective → Plan (feeds the planner) |
| 11 | Skill compounding | 🟡 | Closed loop: Experience → Reflection → Pattern → Candidate skill → **Verification** → Skill Registry → reuse. Only after #5 — verification must exist first, or FRIDAY learns garbage |
| 12 | Egress policy + Secrets vault | 🔴 | Outbound-data gate ("can this secret leave the machine? which provider receives it?") + secrets vault — **before autonomy** (autonomy without egress controls is backwards engineering) |
| 13 | Screen perception | 🔴 | observe UI → understand → act → verify; no camera hardware needed, so it comes first |
| 14 | Audio perception | 🔴 | mic → VAD → ASR → speaker/turn event |
| 15 | Vision perception | 🔴 | camera → frame → vision model → object/event |
| 16 | Persistent Agent Workforce | 🔴 | Assignable task board, persistent per-agent workspaces — requires Agent Director (#7) first |
| 17 | Autonomous Cognitive Tick | 🔴 | Observe → Evaluate → Decide → Act → Verify → Update State, looped. **Last** — needs perception + world model + intent + planning + director + execution + verification + security. Never a blind LLM heartbeat |
| 18 | Observability (OTel/Prometheus) | 🔴 | `/metrics` endpoint consumed by Prometheus |
| 19 | Cost telemetry | 🔴 | Per-model cost accounting ledger, queryable per goal |
| 20 | Walrus Memory backend | optional | Fifth backend behind the existing `MemoryLayer` abstraction — not a missing architectural capability |
| 21 | World-model visualization / DrawDB | last | Sugar; only after the causal world model (#9) is real |

## Milestone gates

1. 🟢 Gate 1 — **Information Organ**: AnyDoc · Web Search · Web Fetch · Evidence ·
   Memory — ✅ **complete (2026-08-10)**.
2. 🟢 Gate 2 — **Trustworthy Executor**: Planning · Execution · Verification ·
   Permissions · Audit · Durability — *the next serious milestone*.
3. 🟢 Gate 3 — **Multi-Agent FRIDAY**: Agent Director · Subagents · Parallel
   workflows · Recovery.
4. 🟢 Gate 4 — **World-Aware FRIDAY**: World Model · Intent · Gap Analysis ·
   Causal Planning.
5. 🟢 Gate 5 — **Learning FRIDAY**: Reflection · Verification · Skill Compounding ·
   Persistent Procedures.
6. 🟢 Gate 6 — **Perceptive FRIDAY**: Screen · Audio · Vision.
7. 🟢 Gate 7 — **Autonomous FRIDAY**: Observe → Reason → Decide → Act → Verify →
   Learn → Repeat.

**Current position: Gate 1 complete; #5 (real local Code/Test Verifier), #6
(the Evidence pipeline — AnyDoc + Web → `Evidence[]` → verification → memory)
and #7 (the Agent Director — bounded role-typed children with gated, verified
results) are done. Gate 2 (Trustworthy Executor) is real: Planning · Execution ·
Verification · Permissions · Audit · Durability are all wired, and Gate 3
(Multi-Agent FRIDAY) has its first brick: the supervisor topology. Next:
#8 parallel durable workflows + agent journaling.**

## Why this order (the corrections vs. the flat P0–P5 table)

- **Stop after `web.fetch` wiring → first real verifier.** Once `web.fetch`
  routes through the resolver, the next unit of work is the first concrete
  verifier. That is the point where FRIDAY stops merely *gaining capabilities*
  and starts gaining **trustworthy execution**.
- **Local verifier before Sonar.** `CodeTestVerifier` proves the verification
  architecture works with zero external services; then Sonar → Security →
  Evidence verifiers slot in behind the same frozen `Verifier` seam
  (`CodeTestVerifier → SonarVerifier → SecurityVerifier → EvidenceVerifier`).
- **Security before autonomy.** Egress policy + secrets move up: "can this
  secret leave the machine?" must be answerable before FRIDAY acts on its own,
  even if the implementation lands incrementally.
- **Agent Director before durable parallelism.** You cannot make durable what
  you have not shaped — topology first, durability second.
- **Skill compounding after verification.** Otherwise the closed loop trains on
  unverified trajectories and FRIDAY learns garbage.
- **Autonomous Tick last.** A heartbeat that blindly calls the LLM is a fancy
  CPU heater, not autonomy. It needs perception + world model + intent +
  planning + director + execution + verification + security first.
- **Walrus down / DrawDB last.** `MemoryLayer` already has multiple real
  implementations — Walrus is an optional fifth backend, not a missing
  capability. DrawDB is sugar until the causal world model exists.

## Update discipline

- When a work item lands (code + tests wired), flip its status to 🟢 and note
  the commit/hash in the Definition of done column.
- Keep the condensed table in the repo [README](../README.md#master-plan--the-corrected-build-order)
  in sync on the same update.
- Before flipping to 🟢, re-verify against the constitution's §2/§3 (Rule 3 —
  classify by evidence): 🟢 means code + tests exist **and are wired**, not that
  a spec or roadmap bullet mentions it.
- Record the change in the [Update log](#update-log).

## Update log

- **2026-08-10** — Plan adopted (corrected master order). Corrections: security
  (egress + secrets) moved ahead of autonomy; local Code/Test verifier before
  Sonar; Agent Director before durable parallelism; skill compounding gated on
  verification; Walrus and DrawDB moved last. Statuses 1–3 🟢 DONE; #4 `web.fetch`
  wiring marked **NEXT**.
- **2026-08-10** — Item #4 `web.fetch` resolver wiring 🟢 DONE (`ResolvingWebTool`
  serves both verbs; fallback + healthy acceptance tests; adapter extracts page
  titles). **Gate 1 (Information Organ) is complete.** Next: #5 the local
  Code/Test Verifier.
- **2026-08-10** — Item #5 real local Code/Test Verifier 🟢 DONE
  (`services/goal-execution/src/verifiers.rs`): first **concrete** (non-mock)
  verifier behind the frozen `Verifier` seam. `run_command_in` captures real
  stdout/stderr/exit code; `CodeTestVerifier::verify_command` runs the plan's
  flow, `to_verification` interprets observed `ToolResult`s; `checks[]` are
  inspectable and a failed check is `Failed`, never `Verified` (incl. the
  FAILED-marker-blocks-false-success acceptance). Constitution §2/§3
  Verification → 🟢. Next: #6 the Evidence pipeline.
- **2026-08-10** — Item #6 Evidence pipeline hardening 🟢 DONE
  (`services/goal-execution/src/evidence_pipeline.rs`): the trustworthy
  research path is real — AnyDoc + Web → unified channel-agnostic `Evidence[]`
  (`EvidenceSet` blends with dedupe + cross-channel corroboration) →
  `EvidenceVerifier` (well-formed/confidence/substance, behind the frozen
  `Verifier` seam) → `evidence_episode` rides the **verified** body into
  episodic memory through the existing admission gate. The executive publishes
  `report.evidence` for evidence-gated plans (acceptance:
  `evidence_pipeline_blends_web_and_docs_into_verified_memory`,
  `executive.rs`). Constitution P1 evidence row → DONE. Next: #7 the Agent
  Director.
- **2026-08-11** — Item #7 Agent Director / bounded subagents 🟢 DONE
  (`services/agent-runtime/src/director.rs`): the supervisor topology above
  `AgentRuntime` — role-typed bounded children (Researcher/Analyst/Verifier,
  `DirectorConfig` pool/deadline/steps), `spawn`/`assign`/`supervise`/`cancel`/
  `collect`/`terminate`/`verify`; every result runs the parent's `ResultGate`
  (`default_gate`) and the goal completes only when
  `DirectorReport::all_verified`. Real-driver acceptance proves the capability
  (`services/action-runtime/tests/agent_director.rs`): Researcher (docs) +
  Verifier (git) through `SkillAgentDriver` + real kernel are both verified,
  and an unapproved verifier blocks the run. `MergedReport::all_succeeded` now
  treats TimedOut/BudgetExhausted as failure (`merge.rs`). Constitution §2/§3
  Recursive agents → 🟢; P1 Agent Director row → DONE. Next: #8 parallel
  durable workflows + agent journaling.
