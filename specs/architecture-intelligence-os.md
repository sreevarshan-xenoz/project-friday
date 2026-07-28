# FRIDAY Architecture — The Intelligence Operating System

**Status:** 🚧 Vision document — informs future specs, does not override frozen contracts.
**Rule:** The event bus, `friday-core` API, `friday-planner` API, and `ActionAdapter`
contracts remain frozen. Everything here is an additive layer above them.

## What FRIDAY is becoming

Not a chatbot. Not an assistant with plugins.

An **operating system for intelligence** — a system that perceives, remembers,
plans, reasons, executes, learns, and expands itself through new skills.

## Architecture

```
                        ┌──────────────────────┐
                        │    Personality Engine │
                        └──────────┬───────────┘
                                   │
                          Conversation Layer
                                   │
                         Intent + Planning Engine
                                   │
         ┌────────────┬────────────┼────────────┬────────────┐
         │            │            │            │            │
    Memory        Skill       World         Reasoning    Tool Selection
    Engine        Engine      Model         Engine       Engine
         │            │            │            │            │
         └────────────┴────────────┼────────────┴────────────┘
                                   │
                          Capability Registry
                                   │
                          Service Orchestrator
                                   │
         ┌────────────┬────────────┼────────────┬────────────┐
         │            │            │            │            │
    Automation    Mission     Learning      Perception   Security
    Engine        Engine      Engine        Engine       Engine
         │            │            │            │            │
         └────────────┴────────────┼────────────┴────────────┘
                                   │
                    Devices · APIs · Models · Robots
                    Browser · Servers · Android · IoT
```

## The Thirteen Engines

### Core (always active)

| # | Engine | Role | Status |
|---|--------|------|--------|
| 1 | **Personality Engine** | Voice, tone, identity, consistency | 🚧 Planned |
| 2 | **Conversation Engine** | Turn-taking, context, multi-turn flow | 🚧 Planned |
| 3 | **Planning Engine** | Goal → intent → plan → execute → verify | ✅ M3 |
| 4 | **Memory Engine** | Working/short/long/semantic/procedural/episodic | ✅ M2 (basic) |
| 5 | **Reasoning Engine** | Tree search, risk analysis, simulation | 🚧 Partial (M4.7) |
| 6 | **Skill Engine** | Modular capabilities, discovery, permissions | 🚧 Spec complete |
| 7 | **Tool Selection Engine** | Rank and select best capability for goal | 🚧 Planned |

### Autonomous (emergent behavior)

| # | Engine | Role | Status |
|---|--------|------|--------|
| 8 | **Automation Engine** | Proactive detection, self-healing, suggestions | 🚧 Planned |
| 9 | **Mission Engine** | Multi-step autonomous goals with recovery | 🚧 Planned |
| 10 | **Learning Engine** | Pattern detection, preference learning, improvement | 🚧 Planned |
| 11 | **Perception Engine** | Camera, screen, speech, clipboard, notifications | 🚧 Planned |
| 12 | **World Model** | Real-time state of system, devices, environment | ✅ Partial (telemetry) |
| 13 | **Security Engine** | Permissions, encryption, audit, authentication | ✅ M4 (basic) |

---

## Detailed Engine Designs

### 1. Personality Engine

FRIDAY should feel like a consistent entity, not a text generator.

```
Personality
├── Voice (tone, formality, humor level)
├── Identity (name, backstory, values)
├── Consistency (never contradicts itself)
├── Adaptation (adjusts to user over time)
└── Context awareness (knows when to be brief vs detailed)
```

**Events consumed:** `conversation.*`, `user.preference.*`
**Events produced:** `personality.style Applied`

**Implementation:** System prompt construction + response post-processing.
The personality is data (a config file), not hardcoded behavior.

---

### 2. Conversation Engine

Multi-turn conversation with context management.

```
Conversation
├── Turn management (who speaks when)
├── Context windowing (what to keep, what to forget)
├── Reference resolution ("it", "that", "the previous one")
├── Topic tracking (what we're discussing)
├── Interruption handling (mid-task new request)
└── Multi-modal input (text + voice + image in one turn)
```

**Events consumed:** `user.input.*`, `speech.transcript`, `camera.frame`
**Events produced:** `conversation.turn`, `intent.raw`

**Implementation:** Extends the existing Intent Engine. Adds conversation state
tracking and context management. The Context Builder already assembles
context; this engine decides *what context to include* per turn.

---

### 3. Planning Engine (existing, enhanced)

Already implemented (M3). Four reasoning engines:
Intent → Planning → Execution → Verification.

**Enhancement path:**
- Tree search for complex goals (multiple solution paths)
- Cost-aware planning (prefer cheap solutions)
- Parallel task execution (DAG-based)

---

### 4. Memory Engine (existing, enhanced)

Already implemented (M2). Working → Session → Long-term → Knowledge Graph → Vector.

**Enhancement path — six memory types:**

| Type | What it stores | Example |
|------|---------------|---------|
| **Working** | Current task context | "Building Docker image, step 3/5" |
| **Short-term** | Recent conversation turns | Last 10 messages |
| **Long-term** | Persistent facts | "User prefers vim over nano" |
| **Semantic** | Knowledge relationships | "Docker uses containers, containers use images" |
| **Procedural** | How to do things | "This machine uses rootless Docker" |
| **Episodic** | What happened when | "Yesterday Docker failed due to permissions" |

**The key insight:** procedural and episodic memory is what makes FRIDAY
learn. After the user fixes a Docker permission issue once, FRIDAY should
never suggest the wrong fix again.

**Events consumed:** `task.completed`, `task.failed`, `verification.*`, `user.correction`
**Events produced:** `memory.stored`, `memory.recalled`

---

### 5. Reasoning Engine

Deep reasoning beyond simple planning.

```
Reasoning
├── Tree search (explore multiple solution paths)
├── Risk analysis (what could go wrong?)
├── Simulation (dry-run before side effects)
├── Causal reasoning (why did this fail?)
├── Analogical reasoning (this is like that other time)
└── Counterfactual reasoning (what if we tried X instead?)
```

**Events consumed:** `plan.created`, `task.failed`, `verification.failed`
**Events produced:** `reasoning.analysis`, `plan.revised`

**Implementation:** Builds on existing Simulation (M4.7). The LLM reasons
over the Capability Registry + World Model + Memory to evaluate plans
before execution. Simulation already virtualizes TaskGraphs; this engine
adds deeper analysis.

---

### 6. Skill Engine (spec complete)

Modular, installable capabilities.

```
Skills
├── Discovery (bundled, user, plugin, remote)
├── Loading (manifest → validate → resolve capabilities)
├── Registry (what's available, what's enabled)
├── Permissions (skill-level approval)
├── Dependencies (skill A requires skill B)
├── Versioning (semver, min core version)
├── Examples (few-shot planning)
└── Runtime toggle (enable/disable without restart)
```

**See:** `specs/skill-engine.md`

**Key principle:** Skills declare *what* can be done. The Tool Selection
Engine decides *which* skill/provider to use for a specific goal.

---

### 7. Tool Selection Engine

The missing piece. The LLM should not manually choose tools.

```
Goal
  │
  ▼
Planner produces plan
  │
  ▼
Tool Selector ranks candidates
  │  ├── success_history
  │  ├── latency
  │  ├── reliability
  │  ├── permissions (user approved?)
  │  ├── platform (compatible?)
  │  ├── cost
  │  └── user_preference
  │
  ▼
Best tool selected
  │
  ▼
Executor runs it
  │
  ▼
Result fed back to Memory (update success rates)
```

**Events consumed:** `plan.action`, `skill.matched`
**Events produced:** `tool.selected`, `tool.ranked`

**Implementation:** Wraps the existing `CapabilityRegistry::ranked_providers_for()`
with skill-awareness and learning. The registry already ranks by
`reliability*100 - latency*0.05 - cost`; this engine adds:
- Success history from Memory
- User preference signals
- Skill context (which skill is active?)
- Permission status (approved or needs prompting?)

**The feedback loop:** After execution, the result (success/failure, latency)
is fed back into Memory, which updates the ranking weights. FRIDAY gets
better at choosing tools over time.

---

### 8. Automation Engine

Proactive behavior instead of waiting for commands.

```
Automation
├── Detect problems (service down, disk full, process stuck)
├── Self-heal (restart service, clear cache, kill zombie)
├── Suggest improvements ("You always do X, want me to automate it?")
├── Monitor thresholds (CPU > 90%, disk > 85%)
├── Pattern detection (repetitive manual work)
└── Explain actions ("I restarted Docker because it was unresponsive")
```

**Events consumed:** `telemetry.system`, `service.health.*`, `task.failed`
**Events produced:** `automation.detected`, `automation.suggested`, `automation.action`

**Implementation:** Extends ServiceOrchestrator's existing health monitoring.
Currently the orchestrator probes and restarts unhealthy services. The
Automation Engine adds:
- Threshold-based alerts
- Pattern detection across episodes
- User-facing suggestions (never auto-execute sensitive actions)
- Explanation generation ("I did X because Y")

---

### 9. Mission Engine

Multi-step autonomous goals.

```
Mission
├── Goal decomposition (mission → goals → tasks)
├── Progress tracking (what's done, what's next)
├── Failure recovery (retry, replan, ask user)
├── Parallel execution (independent subtasks)
├── Milestone verification (checkpoints)
└── Completion reporting (what was accomplished)
```

**Events consumed:** `mission.created`, `task.completed`, `task.failed`
**Events produced:** `mission.progress`, `mission.completed`, `mission.failed`

**Implementation:** Extends the existing Planner + GoalSupervisor. The
GoalSupervisor already tracks goal lifecycles; the Mission Engine adds:
- Hierarchical decomposition (mission → goals → tasks → actions)
- Checkpoint/rollback capability
- Multi-day missions (persist across sessions)
- Progress notifications to the user

**Example:**
```
Mission: "Set up the new project"
  ├── Goal: Create GitHub repo
  │   └── Task: gh repo create ...
  ├── Goal: Initialize project structure
  │   ├── Task: cargo init / npm init
  │   ├── Task: Add README
  │   └── Task: Add .gitignore
  ├── Goal: Set up CI
  │   └── Task: Create GitHub Actions workflow
  └── Goal: Deploy to production
      ├── Task: Build
      ├── Task: Test
      └── Task: Deploy
```

---

### 10. Learning Engine

FRIDAY improves without explicit programming.

```
Learning
├── Preference learning ("user always chooses vim")
├── Workflow detection ("user starts VS Code → Docker → browser")
├── Error patterns ("this fix always works for this error")
├── Optimization ("faster way to do what user asked")
├── Personalization ("user prefers brief responses")
└── Knowledge accumulation ("learned about project X from conversation Y")
```

**Events consumed:** `task.completed`, `user.correction`, `conversation.turn`
**Events produced:** `learning.insight`, `learning.preference`, `learning.workflow`

**Implementation:** Reads from Memory Engine's episodic and procedural stores.
Detects patterns across episodes:
- Sequence detection: "User always does A → B → C"
- Preference detection: "User always rejects option A"
- Error correlation: "This error always has this fix"
- Efficiency detection: "There's a faster way to do this"

The Learning Engine writes insights back to Memory as procedural memories.
The next time a similar situation arises, FRIDAY already knows.

---

### 11. Perception Engine

Sensory input from the environment.

```
Perception
├── Desktop (screen content, window titles, active app)
├── Camera (face, objects, gestures, OCR)
├── Speech (wake word, streaming recognition, emotion)
├── Clipboard (text, images, files)
├── Notifications (system, app-specific)
├── Files (new/modified/deleted)
├── Network (connections, latency, packets)
└── Android (screen, sensors, GPS)
```

**Events consumed:** `sensor.*` (raw input from devices)
**Events produced:** `perception.desktop`, `perception.camera`, `perception.speech`,
`perception.clipboard`, `perception.notification`

**Implementation:** Each sensor is a plugin that produces `perception.*` events.
The Perception Engine normalizes and routes them. The World Model consumes
perception events to maintain state. The Planner can request perception
data as context.

---

### 12. World Model

Real-time state of everything FRIDAY can observe.

```
World Model
├── System state (CPU, RAM, disk, network, processes)
├── Device state (USB, Bluetooth, display, audio)
├── Application state (running apps, windows, tabs)
├── Network state (connected, latency, bandwidth)
├── Environment (weather, time, location)
├── Service state (Docker, servers, databases)
├── User state (active, idle, in meeting, sleeping)
└── Project state (git status, build status, test results)
```

**Events consumed:** `telemetry.*`, `perception.*`, `service.health.*`
**Events produced:** `world.state`, `world.changed`

**Implementation:** Extends existing `SystemProbe` (telemetry) + ServiceOrchestrator
(health). The World Model aggregates all signals into a unified state store.
The Planner and Reasoning Engine query the World Model for situational context.

**Key difference from telemetry:** Telemetry is raw numbers. The World Model
interprets them: "CPU is high because Docker is building an image" not just
"CPU is 85%".

---

### 13. Security Engine

Permission, audit, and safety layer.

```
Security
├── Permission system (per-action, per-skill, per-user)
├── Encryption (local secrets, at-rest, in-transit)
├── Audit log (every action recorded)
├── Multi-user profiles (different permissions per user)
├── Voice/face authentication
├── Dangerous action gating (delete, network, admin)
└── Anomaly detection (unusual behavior alerts)
```

**Events consumed:** `exec.action`, `tool.selected`, `user.auth.*`
**Events produced:** `security.approved`, `security.denied`, `security.audit`

**Implementation:** Extends existing permission layer (M4). The Security Engine
adds multi-user support, biometric auth, and anomaly detection. Every
action passes through Security before execution.

---

## Implementation Sequence

Build in dependency order. Each stage increases capability while building
on the previous one.

```
Phase 1: Foundation (what exists + Skill Engine)
├── ✅ Event bus, plugin system, protocol transport
├── ✅ Memory (basic layers)
├── ✅ Planner (4 engines)
├── ✅ Capability Intelligence + Registry
├── ✅ Service Orchestrator
├── ✅ Context Builder
├── ✅ Intelligence Layer (LLM)
├── ✅ Recovery + Simulation
├── ✅ Mobile Protocol + Android Client
├── ✅ Desktop UI (React + TypeScript)
├── 🚧 Skill Engine (spec complete)
└── 🚧 Tool Selection Engine (spec this phase)

Phase 2: Intelligence
├── Enhanced Memory Engine (6 memory types)
├── World Model (system + device state)
├── Reasoning Engine (tree search + risk analysis)
└── Learning Engine (pattern detection)

Phase 3: Autonomy
├── Mission Engine (multi-step goals)
├── Automation Engine (proactive behavior)
├── Perception Engine (sensors + input)
└── Security Engine (multi-user + biometric)

Phase 4: Integration
├── Personality Engine (consistent identity)
├── Conversation Engine (multi-turn + context)
└── Cross-engine learning (all engines feed Memory)
```

## Core Invariants (unchanged)

1. The core contains **no intelligence**. All behavior is in engines/plugins.
2. Nothing is reachable without going through the event bus or transport.
3. Dangerous actions pass through the permission layer before execution.
4. Every new feature answers: **which event does it consume? which does it produce? which engine owns it?**
5. **Skills are data, not code.** Adding a skill = adding a manifest, not changing the core.

## What makes fictional assistants feel intelligent

It's not one system. It's the interaction:

| Feeling | Engine responsible |
|---------|-------------------|
| "It remembers me" | Memory Engine |
| "It knows what's happening" | World Model + Perception |
| "It plans before acting" | Planning + Reasoning |
| "It learns my patterns" | Learning Engine |
| "It acts on its own" | Automation + Mission |
| "It has a personality" | Personality Engine |
| "It uses the right tool" | Tool Selection + Skill |
| "It stays safe" | Security Engine |

All thirteen engines working together. That's the operating system for intelligence.
