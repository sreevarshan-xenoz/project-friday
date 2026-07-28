# FRIDAY — Final Architecture

**Status:** 🚧 Vision document — the target architecture for FRIDAY as an
Adaptive Cognitive Operating System. Supersedes `architecture-intelligence-os.md`.
**Rule:** Event bus, `friday-core` API, `friday-planner` API, and `ActionAdapter`
contracts remain frozen. Everything here is additive.

**⚠️ Architecture is complete.** Stop writing specs. Start building and measuring.
The evaluation framework (`evaluation-framework.md`) defines how we know
if it's working. Build → Measure → Improve → Write. Not Read → Read → Read → New architecture.

## What FRIDAY is

Not a chatbot. Not an assistant with plugins. Not a consciousness simulation.

An **Adaptive Cognitive Operating System** — a system that continuously observes,
understands, plans, acts, verifies, reflects, and improves. It does not wait for
commands. It operates in a cognitive loop. It is measurable by its capabilities,
not by philosophical claims.

**Design goal:** A general-purpose cognitive platform that can host many different
assistant personalities and capabilities on the same underlying architecture.

## Core Invariant: Single Responsibility

Every engine owns **exactly one responsibility**. No God classes.

| Engine | Owns | Does NOT own |
|--------|------|-------------|
| Memory Engine | Storing, retrieving, forgetting experiences | Planning, tool selection, emotions |
| Reasoning Engine | Hypothesis generation, simulation, evaluation | Execution, planning |
| Agent Runtime | Retries, rollback, checkpoints, budget | Deciding what to execute |
| Planning Engine | Goal decomposition, step ordering | Execution, reflection |
| World Model | Graph state, relationships, queries | Planning, execution |
| Knowledge Engine | Facts, docs, code index, ontology | Experiences, planning |
| Learning Engine | Pattern detection, preference learning | Execution, memory storage |
| Skill Engine | Capability declarations, permissions | Prompts, instructions, personality |
| Tool Selector | Choosing which tool to use | Choosing which model to use |
| Provider Engine | Choosing which model to use | Choosing which tool to use |
| Reflection Engine | Post-mission analysis, lesson extraction | Memory storage, learning patterns |

**Violation test:** If you find an engine doing two things, split it.
If you find an engine reaching into another engine's responsibility, stop.
Complexity explosion is the enemy. Every engine talks to the bus, not to
every other engine directly.

## The Cognitive Loop

Every engine plugs into a continuous cycle:

```
     ┌──────────────────────────────────────────────┐
     │                                              │
     ▼                                              │
  OBSERVE ──→ UNDERSTAND ──→ PLAN ──→ ACT ──→ VERIFY ──→ LEARN
     ▲                                              │
     │                                              │
     └──────────────────────────────────────────────┘
```

This is not a pipeline that starts and stops. It is a **heartbeat**.
FRIDAY is always observing. Always understanding. Always ready to plan.
Always learning from what happened.

The **Cognitive Runtime** owns this loop. Every other engine plugs into it.

## Architecture

```
                               ┌────────────────┐
                               │  Personality   │
                               └───────┬────────┘
                                       │
                            Conversation Engine
                                       │
                               Cognitive Runtime
                               (Global Workspace)
                                       │
         ┌──────────────┬──────────────┼──────────────┬──────────────┐
         │              │              │              │              │
    Memory         Knowledge       World          Reasoning      Learning
    Engine         Engine          Model          Engine         Engine
         │              │              │              │              │
         └──────────────┴──────────────┼──────────────┴──────────────┘
                                       │
                               Planning Engine
                                       │
                               Mission Engine
                                       │
                               Agent Runtime
                                       │
         ┌──────────────┬──────────────┼──────────────┬──────────────┐
         │              │              │              │              │
    Skill Engine   Tool Selector   Provider      Perception     Security
                                       Engine      Engine         Engine
         │              │              │              │              │
         └──────────────┴──────────────┼──────────────┴──────────────┘
                                       │
                             Capability Registry
                                       │
                             Service Orchestrator
                                       │
                    Desktop · Android · Browser · APIs
                    Docker · IoT · Servers · Robots

                         ┌──────────────────────┐
                         │  Reflection Engine    │
                         │  (post-mission)       │
                         └──────────────────────┘
```

## Goal Hierarchy

Every piece of work flows through a clear hierarchy:

```
Goal                     "Build the FRIDAY website"
  │
  ├── Mission            "Frontend" · "Backend" · "Deploy"
  │     │
  │     ├── Plan         "Task 1" · "Task 2" · "Task 3"
  │     │     │
  │     │     └── Execute   "Run terminal" · "Run git" · "Run docker"
  │     │
  │     └── Plan         ...
  │
  └── Mission            ...
```

| Level | Owns | Example |
|-------|------|---------|
| **Goal** | High-level intent from user or autonomous detection | "Build the FRIDAY website" |
| **Mission** | Decomposed goals with milestones | "Frontend", "Backend", "Deploy" |
| **Plan** | Ordered steps (DAG) for one mission | "Task 1 → Task 2 → Task 3" |
| **Execution** | Concrete actions via tools | "cargo build", "git push" |

This hierarchy scales from "open notepad" to "launch a product."

## The Eighteen Engines

### Layer 0: Identity

| # | Engine | Role |
|---|--------|------|
| 1 | **Personality Engine** | Voice, tone, identity, consistency |

### Layer 1: Cognition

| # | Engine | Role |
|---|--------|------|
| 2 | **Conversation Engine** | Multi-turn flow, context management, references |
| 3 | **Cognitive Runtime** | The loop: observe → understand → plan → act → verify → reflect → learn |
| 4 | **Reasoning Engine** | Tree search, risk analysis, simulation, causal reasoning |
| 5 | **Knowledge Engine** | Facts, documentation, APIs, codebase indexing, ontology (distinct from memory) |

### Layer 2: State

| # | Engine | Role |
|---|--------|------|
| 6 | **Memory Engine** | Working/short/long/semantic/procedural/episodic (experiences) |
| 7 | **World Model** | Real-time graph of system, devices, environment, relationships |
| 8 | **Learning Engine** | Pattern detection, preference learning, workflow optimization |

### Layer 3: Planning

| # | Engine | Role |
|---|--------|------|
| 9 | **Planning Engine** | Goal → intent → plan → execute → verify (existing, enhanced) |
| 10 | **Mission Engine** | Multi-step autonomous goals with checkpoints and recovery |

### Layer 4: Execution

| # | Engine | Role |
|---|--------|------|
| 11 | **Agent Runtime** | Retries, checkpoints, cancellation, rollback, timeout, recovery |
| 12 | **Skill Engine** | Modular capability declarations, discovery, permissions |
| 13 | **Tool Selector** | Ranks and selects best provider for each action |
| 14 | **Provider Engine** | Selects best model/provider for each task (LLM routing) |

### Layer 5: Interface

| # | Engine | Role |
|---|--------|------|
| 15 | **Perception Engine** | Camera, screen, speech, clipboard, notifications, sensors |
| 16 | **Security Engine** | Permissions, encryption, audit, authentication |
| 17 | **Automation Engine** | Proactive detection, self-healing, suggestions |
| 18 | **Reflection Engine** | Post-mission analysis, lesson extraction, causal review |

---

## Detailed Engine Designs

### 1. Personality Engine

FRIDAY should feel like a consistent entity.

```
Personality
├── Voice (tone, formality, humor level)
├── Identity (name, backstory, values)
├── Consistency (never contradicts itself)
├── Adaptation (adjusts to user over time)
└── Context awareness (brief vs detailed based on situation)
```

**Implementation:** System prompt construction + response post-processing.
Personality is data (config), not code.

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

**Implementation:** Extends the Intent Engine. Adds conversation state
tracking. The Context Builder decides *what context to include* per turn.

---

### 3. Cognitive Runtime ← Global Workspace

The heartbeat. Not a single engine but the **loop that connects all engines**.
Uses the **Global Workspace** pattern: every engine "listens," only relevant
engines wake up.

```
     ┌──────────────────────────────────────────────┐
     │              Cognitive Runtime               │
     │                                              │
     │  OBSERVE ──→ UNDERSTAND ──→ PLAN ──→ ACT    │
     │     ▲                              │         │
     │     └──────── LEARN ◄── VERIFY ◄───┘         │
     │                                              │
     │  Cycle rate: configurable (default 100ms)    │
     │  Always running. Always observing.           │
     └──────────────────────────────────────────────┘
```

**What it owns:**
- Cycle timing (how often the loop runs)
- Engine coordination (which engine handles which phase)
- Priority arbitration (when multiple things need attention)
- State transitions (idle → observing → planning → acting)
- Interrupt handling (urgent events break the current cycle)

**Global Workspace pattern:**

```
Perception
    │
    ▼
Workspace (shared state)
    │
    ▼
Attention (what matters right now?)
    │
    ▼
Broadcast (notify relevant engines)
    │
    ▼
Engines (only those that match wake up)
```

Every engine registers interest in specific event namespaces.
The workspace broadcasts. Only matching engines process.
This scales: 17 engines or 170, the bus handles it.

**Cycle phases:**

| Phase | What happens | Engines involved |
|-------|-------------|-----------------|
| **Observe** | Gather signals from sensors, telemetry, events | Perception, World Model |
| **Understand** | Interpret what's happening, detect patterns | Knowledge, Memory, Learning |
| **Plan** | Decide what to do (if anything) | Reasoning, Planning, Mission |
| **Act** | Execute the plan | Agent Runtime, Tool Selector, Skills |
| **Verify** | Confirm the action succeeded | Agent Runtime, Planning |
| **Reflect** | Post-action analysis, lesson extraction | Reflection Engine |
| **Learn** | Record what happened, update knowledge | Memory, Learning, Knowledge |

**Key insight:** Without the Cognitive Runtime, engines react to events
but never feel like they're "thinking." With it, FRIDAY is always
processing, always aware, always ready.

**Events consumed:** `sensor.*`, `telemetry.*`, `user.input.*`, `task.*`
**Events produced:** `cognitive.cycle`, `cognitive.phase.*`, `cognitive.priority`,
`cognitive.workspace.broadcast`, `cognitive.attention.focus`

---

### 4. Reasoning Engine

Deep reasoning beyond planning.

```
Reasoning
├── Tree search (multiple solution paths)
├── Risk analysis (what could go wrong?)
├── Simulation (dry-run before side effects)
├── Causal reasoning (why did this fail?)
├── Analogical reasoning (like that other time)
└── Counterfactual (what if we tried X?)
```

**Implementation:** Builds on Simulation (M4.7). Reasons over World Model
+ Knowledge + Memory.

---

### 5. Knowledge Engine ← Split into sub-components

Distinct from Memory. Memory is experiences. Knowledge is facts.
At scale, one monolithic knowledge store becomes unwieldy. Split into:

```
Knowledge
├── Reference        Static documentation, API refs, manuals
├── Code Index       Function signatures, types, module structure
├── Concept Graph    Relationships (Docker → containers → images → layers)
├── Ontology         Formal structure (RDF, property graphs, SKOS)
└── Learned Facts    Extracted from conversations and outcomes
```

**Memory vs Knowledge:**

| Memory | Knowledge |
|--------|-----------|
| "Sree prefers Arch Linux" | "Cargo builds Rust projects" |
| "Docker failed yesterday" | "Docker uses OCI images" |
| "User always starts with vim" | "Rust Analyzer is an LSP" |
| Personal experiences | Universal facts |
| Changes over time | Stable (until updated) |

**Knowledge sources:**
- Local: project READMEs, code comments, config files
- Remote: package registries, documentation sites
- User-provided: notes, bookmarks, PDFs
- Learned: facts extracted from conversations

**Sub-component responsibilities:**

| Sub-component | Owns | Query pattern |
|--------------|------|---------------|
| **Reference** | Static docs, API refs, manuals | "What is X?" |
| **Code Index** | Function signatures, types, modules | "Where is X defined?" |
| **Concept Graph** | Relationship traversal | "What depends on X?" |
| **Ontology** | Formal structure, classification | "What kind of X is Y?" |
| **Learned Facts** | Extracted knowledge | "What did we learn about X?" |

**Events consumed:** `knowledge.query`, `file.changed`, `conversation.turn`
**Events produced:** `knowledge.found`, `knowledge.indexed`, `concept.linked`

**Implementation:** Each sub-component can use the最适合 storage:
- Reference: embedding store for semantic search
- Code Index: AST parsing + symbol table
- Concept Graph: property graph (Neo4j-style)
- Ontology: RDF/OWL or property graph with formal edges
- Learned Facts: key-value store with TTL

---

### 6. Memory Engine (existing, enhanced)

Six memory types:

| Type | What | Example |
|------|------|---------|
| Working | Current task context | "Building Docker image, step 3/5" |
| Short-term | Recent conversation | Last 10 messages |
| Long-term | Persistent facts | "User prefers vim" |
| Semantic | Knowledge relationships | "Docker uses containers" |
| Procedural | How to do things | "This machine uses rootless Docker" |
| Episodic | What happened when | "Yesterday Docker failed due to permissions" |

**Key insight:** Procedural + Episodic memory is what makes FRIDAY learn.
After the user fixes a Docker permission issue once, FRIDAY should never
suggest the wrong fix again.

---

### 7. World Model ← Graph-based

Not a flat list. A **connected graph**.

```
Laptop
├── CPU (85%)
├── GPU (RTX 3060)
├── RAM (12.4 / 32 GB)
├── Docker
│   ├── Minecraft Server
│   │   ├── Port: 25565
│   │   ├── Players: 3/20
│   │   └── Uptime: 14h
│   ├── PostgreSQL
│   │   ├── Database: friday_db
│   │   └── Connections: 5
│   └── Redis
│       └── Memory: 256 MB
├── Network
│   ├── WiFi: Connected (54 Mbps)
│   └── VPN: Disconnected
├── Bluetooth
│   ├── Headphones: Connected
│   └── Keyboard: Connected
├── Git
│   └── project-friday
│       ├── Branch: dev
│       ├── Modified: 12 files
│       └── Last commit: 2h ago
└── User
    ├── Active: Yes
    ├── Idle: 0 min
    └── Focus: VS Code
```

**Every node can query its children:**
- "What's running in Docker?" → traverse Docker node
- "What's the Minecraft server status?" → Docker → Minecraft
- "What's the git state?" → Git → project-friday

**Graph edges carry relationships:**
- `Docker → CONTAINS → Minecraft`
- `Minecraft → USES → Port 25565`
- `User → WORKING_ON → project-friday`
- `project-friday → DEPLOYED_VIA → Docker`

**Events consumed:** `telemetry.*`, `perception.*`, `service.health.*`
**Events produced:** `world.state`, `world.node.updated`, `world.edge.added`

**Implementation:** In-memory graph with periodic persistence. The Reasoning
Engine traverses the graph for causal analysis. The Context Builder extracts
relevant subgraphs for LLM context.

---

### 8. Learning Engine

FRIDAY improves without explicit programming.

```
Learning
├── Preference detection ("user always chooses vim")
├── Workflow detection ("VS Code → Docker → browser → Spotify")
├── Error correlation ("this error always has this fix")
├── Efficiency detection ("faster way to do this")
├── Personalization ("user prefers brief responses")
└── Knowledge extraction ("learned X from conversation Y")
```

**Events consumed:** `task.completed`, `user.correction`, `conversation.turn`
**Events produced:** `learning.insight`, `learning.preference`, `learning.workflow`

**Implementation:** Reads Memory's episodic + procedural stores. Detects
patterns across episodes. Writes insights back as procedural memories.

---

### 9. Planning Engine (existing, enhanced)

Four reasoning engines: Intent → Planning → Execution → Verification.

**Enhancement:** Tree search for complex goals, cost-aware planning,
parallel task execution via DAG.

---

### 10. Mission Engine

Multi-step autonomous goals with checkpoints.

```
Mission
├── Goal decomposition (mission → goals → tasks)
├── Progress tracking (what's done, what's next)
├── Checkpoint/rollback (save state, resume later)
├── Parallel execution (independent subtasks)
├── Multi-day persistence (survive restarts)
└── Completion reporting
```

**Events consumed:** `mission.created`, `task.completed`, `task.failed`
**Events produced:** `mission.progress`, `mission.completed`, `mission.failed`

---

### 11. Agent Runtime ← NEW

The execution engine that owns the lifecycle of actions. Not the planner.
Not the mission engine. The **runtime**.

```
Agent Runtime
├── Retries (with exponential backoff)
├── Checkpoints (save state at each step)
├── Cancellation (user can stop anytime)
├── Pause/Resume (freeze and continue later)
├── Rollback (undo to last checkpoint)
├── Timeout (kill stuck actions)
├── Progress tracking (% complete)
├── Recovery (automatic failure handling)
├── Budget tracking (time, tokens, cost)
└── Audit trail (every action recorded)
```

**Example flow:**

```
Goal: Build Android APK
  │
  ▼
Step 1: cargo build --release
  │ Result: ✓ (32s)
  ▼
Step 2: Gradle assembleDebug
  │ Result: ✕ (compile error)
  ▼
Agent Runtime:
  ├── Log failure
  ├── Check retry budget (2/3 remaining)
  ├── Retry with same command
  │   Result: ✕ (same error)
  ├── Search error in Knowledge Engine
  │   Found: "missing ANDROID_HOME"
  ├── Apply fix: set environment variable
  ├── Retry with fix
  │   Result: ✓ (45s)
  ├── Record: "this error = set ANDROID_HOME" → Memory
  └── Continue to Step 3
```

**Events consumed:** `plan.action`, `exec.result`
**Events produced:** `runtime.action.started`, `runtime.action.completed`,
`runtime.action.failed`, `runtime.retry`, `runtime.rollback`,
`runtime.checkpoint`

**Implementation:** Wraps the existing `AgentKernel` in `friday-actions`.
The kernel already handles execution; the Agent Runtime adds lifecycle
management (retry, checkpoint, rollback, budget, audit).

---

### 12. Skill Engine (spec complete, updated)

Modular capability **declarations**. No prompts.

```
Skills
├── Manifest (capabilities, actions, permissions)
├── Discovery (bundled, user, plugin, remote)
├── Loading (validate → resolve → register)
├── Permissions (skill-level approval)
├── Dependencies (skill A requires skill B)
├── Versioning (semver, min core version)
└── Runtime toggle (enable/disable)
```

**Skills do NOT contain:**
- Prompts (belong in Context Builder)
- Instructions (belong in planner)
- Personality (belong in Personality Engine)

**Skills DO contain:**
- Capability IDs (what registry capabilities they need)
- Action descriptions (natural language for planner, used for goal matching)
- Permission requirements (what needs user approval)
- Dependencies, platform, version constraints

**Prompt construction happens in the Context Builder**, which reads skill
metadata and formats it for the LLM. This keeps skills as pure data
and avoids maintenance burden.

**Skills do NOT contain prompts, instructions, or personality.** They are
declarations of capability, not behavior. The Context Builder constructs
the LLM prompt using skill data as input.

---

### 13. Tool Selector

Ranks and selects the best provider for each action.

```
Tool Selection
├── Candidate scoring (reliability, latency, cost, preference)
├── Skill context bonus (active skill's providers rank higher)
├── Permission awareness (approved tools preferred)
├── User preference learning (accepted/rejected history)
├── Feedback loop (execute → record → learn → better selection)
└── Tie-breaking (recency, experience, speed)
```

**Events consumed:** `plan.action`, `skill.matched`
**Events produced:** `tool.selected`, `tool.ranked`, `tool.executed`

---

### 14. Provider Engine ← NEW

The Tool Selector chooses *tools*. The Provider Engine chooses *models*.

```
Provider Engine
├── Model routing (which model for which task)
├── Cost optimization (cheapest model that works)
├── Latency optimization (fastest model for quick tasks)
├── Quality optimization (best model for complex reasoning)
├── Offline fallback (local model when cloud unavailable)
├── Multi-provider support (OpenAI, Anthropic, Gemini, Ollama, ...)
├── Load balancing (distribute across providers)
├── Failover (backup provider if primary fails)
├── Capacity tracking (rate limits, daily quotas)
├── Quality scoring (track which provider gave best results)
├── Cost budgeting (daily/monthly limits with user approval)
└── Health monitoring (latency, error rate, uptime)
```

**Routing rules:**

| Task type | Provider | Reason |
|-----------|----------|--------|
| Image generation | Gemini | Best multimodal |
| Code generation | Claude | Best code quality |
| Math/logic | OpenAI | Best reasoning |
| Offline/quick | Ollama (local) | No latency, no cost |
| Simple query | Gemma/Phi (local) | Fast, cheap |
| Translation | Qwen | Best multilingual |

**Selection algorithm:**
1. Filter models by capability requirements (vision, function calling, etc.)
2. Filter by context window >= required context length
3. Filter by quality score >= threshold
4. Rank by: quality_score × (1 / cost) × (1 / latency)
5. Check capacity: is rate limit available?
6. Return top-ranked model with available capacity
7. If no model meets requirements, fallback to best available

**Failover chain:**
```
Primary: OpenAI GPT-4o
  ↓ timeout / rate limit / error
Fallback 1: Anthropic Claude Sonnet 4
  ↓ timeout / rate limit / error
Fallback 2: Local Ollama llama3.1:8b
  ↓ all providers failed
Escalate to user
```

**Events consumed:** `plan.action`, `llm.request`
**Events produced:** `provider.selected`, `provider.routed`, `provider.failover`,
`provider.health.*`, `provider.capacity.*`, `provider.budget.*`, `provider.quality.*`

**Implementation:** Extends existing `friday-intelligence` (providers + router).
The Provider Engine adds:
- Task-aware routing (not just "pick the cheapest")
- Offline/online switching
- Cost budget enforcement
- Quality feedback loop (track which provider gave best results)
- Capacity tracking (prevent rate limit violations)
- Health monitoring (detect unhealthy providers)

---

### 15. Perception Engine

Sensory input from the environment.

```
Perception
├── Desktop (screen content, window titles, active app)
├── Camera (face, objects, gestures, OCR)
├── Speech (wake word, streaming recognition, emotion)
├── Clipboard (text, images, files)
├── Notifications (system, app-specific)
├── Files (new/modified/deleted)
├── Network (connections, latency)
└── Android (screen, sensors, GPS)
```

**Events consumed:** `sensor.*`
**Events produced:** `perception.desktop`, `perception.camera`, `perception.speech`

---

### 16. Security Engine

Permission, audit, and safety.

```
Security
├── Permission system (per-action, per-skill, per-user)
├── Encryption (secrets vault, at-rest, in-transit)
├── Audit log (every action recorded)
├── Multi-user profiles
├── Biometric authentication (voice, face)
├── Dangerous action gating
└── Anomaly detection
```

---

### 17. Automation Engine

Proactive behavior.

```
Automation
├── Problem detection (service down, disk full)
├── Self-heal (restart, clear cache, kill zombie)
├── Suggest improvements ("want me to automate this?")
├── Pattern detection (repetitive manual work)
├── Threshold monitoring (CPU > 90%)
└── Explanation generation ("I did X because Y")
```

---

### 18. Reflection Engine ← NEW

After every mission, a post-mortem. Not memory. Not learning. **Reflection.**

```
Reflection
├── Mission completed → What worked?
├── Mission failed → What broke?
├── Error encountered → What was the root cause?
├── Recovery succeeded → What fix worked?
├── Time analysis → Where did we spend the most time?
├── Lesson extraction → Store as procedural knowledge
└── Plan improvement → What should we do differently?
```

**Why this is separate from Memory and Learning:**

| Component | When | What |
|-----------|------|------|
| **Memory** | During execution | Records what happened (episodes) |
| **Learning** | Periodically | Detects patterns across episodes |
| **Reflection** | After each mission | Analyzes why things happened, extracts lessons |

**Reflection flow:**

```
Mission finished
    │
    ▼
Reflection Engine
    ├── Read mission checkpoints and execution history
    ├── Read error logs and recovery records
    ├── Analyze: what steps took longest?
    ├── Analyze: where did failures cluster?
    ├── Extract: "this error = this fix" → procedural knowledge
    ├── Extract: "this approach works better" → workflow improvement
    ├── Store lessons in Memory (procedural)
    ├── Update World Model (if environment facts changed)
    └── Emit: reflection.completed with insights
```

**Example:**

```
Rust build failed
    → Cargo.lock corrupted
    → Deleting Cargo.lock fixed issue
    → Reflection: "Corrupted Cargo.lock → delete and rebuild"
    → Stored as procedural knowledge
    → Next time: Planner already knows
```

**Events consumed:** `mission.completed`, `mission.failed`, `runtime.checkpoint`
**Events produced:** `reflection.completed`, `reflection.lesson`, `reflection.insight`

**Key insight:** Reflection is the bridge between doing and learning.
Without it, FRIDAY records events (Memory) and detects patterns (Learning)
but never asks *why*. Reflection asks why, and stores the answer.

---

## Event Bus — Namespaced

Event kinds are namespaced to prevent collision and ease debugging.
Each engine subscribes only to its namespace. The bus filters by prefix
before dispatch, reducing unnecessary processing.

```
cognitive.*          Cognitive Runtime lifecycle
  cognitive.cycle.*      cycle started/completed
  cognitive.phase.*      phase started/completed
  cognitive.interrupted  urgent interrupt
  cognitive.priority.*   priority escalation

conversation.*       Conversation turns, context
  conversation.turn.*    turn started/completed
  conversation.context.* context updates

memory.*             Memory store/recall
  memory.store.*         write operations
  memory.recall.*        read operations
  memory.prune.*         cleanup

knowledge.*          Knowledge query/index
  knowledge.query.*      search operations
  knowledge.indexed      new entry indexed
  knowledge.found        search results
  knowledge.graph.*      graph edge operations
  knowledge.expired      entry expired
  knowledge.source.*     source updates

world.*              World Model state changes
  world.node.*           node add/update/remove
  world.edge.*           edge add/update/remove
  world.state.*          state snapshots

planning.*           Planner events (existing)
  plan.created           new plan
  plan.action.*          action lifecycle
  plan.replan            replanning triggered

mission.*            Mission lifecycle
  mission.created        new mission
  mission.progress       progress update
  mission.completed      mission done
  mission.failed         mission failed

runtime.*            Agent Runtime
  runtime.action.*       action started/completed/failed
  runtime.retry          retry attempt
  runtime.checkpoint     checkpoint saved
  runtime.rollback       rollback triggered
  runtime.cancelled      user cancellation
  runtime.paused/resumed pause lifecycle
  runtime.timeout        action timeout
  runtime.budget.*       budget exceeded
  runtime.progress       execution progress

skill.*              Skill load/enable/disable
  skill.scanned          discovery complete
  skill.loaded           manifest loaded
  skill.enabled/disabled toggle
  skill.removed          uninstalled
  skill.error            loading error
  skill.activated        skill matched to goal

tool.*               Tool selection and execution
  tool.selected          tool chosen
  tool.ranked            ranking update
  tool.executed          tool ran
  tool.feedback          user feedback

provider.*           Model routing
  provider.discovered     provider found
  provider.connected/disconnected lifecycle
  provider.health.*       health check
  provider.model.*        model selection/failure
  provider.failover       failover triggered
  provider.capacity.*     rate limit tracking
  provider.budget.*       cost budget
  provider.cost.*         cost recording
  provider.quality.*      quality scoring

perception.*         Sensor input
  perception.desktop     screen content
  perception.camera      camera input
  perception.speech      voice input
  perception.clipboard   clipboard
  perception.file        file changes

security.*           Permission, audit
  security.permission.*  permission request/grant/deny
  security.audit.*       audit log
  security.auth.*        authentication

automation.*         Proactive detection
  automation.problem     problem detected
  automation.suggest     suggestion generated
  automation.selfheal    self-healing action

learning.*           Pattern detection
  learning.insight       new insight
  learning.preference    preference detected
  learning.workflow      workflow pattern

user.*               User input, corrections
  user.input.*           text/voice/image input
  user.correction        correction received
  user.feedback          explicit feedback

system.*             Telemetry, health
  system.health.*        service health
  system.telemetry.*     metrics
  system.startup/shutdown lifecycle
```

---

## Metrics — Every Engine

Every engine exposes metrics for observability:

```rust
pub struct EngineMetrics {
    pub engine_name: String,
    pub cycle_count: u64,
    pub success_rate: f64,
    pub average_latency_ms: f64,
    pub error_count: u64,
    pub last_active: DateTime<Utc>,
    pub custom: HashMap<String, f64>,
}
```

**Per-engine metrics:**

| Engine | Key Metrics |
|--------|------------|
| Cognitive Runtime | Cycle rate, phase distribution, priority conflicts, workspace broadcast count |
| Planning Engine | Success rate, average plan length, replan count |
| Agent Runtime | Retry rate, checkpoint count, rollback count, budget usage |
| Tool Selector | Selection accuracy, average latency, preference learning rate |
| Provider Engine | Cost per task, latency p95, failover rate, quality scores |
| Memory Engine | Recall rate, retrieval latency, storage size |
| Knowledge Engine | Index size, query latency, relevance score, sub-component sizes |
| World Model | Node count, edge count, update frequency, staleness |
| Learning Engine | Pattern detection rate, preference accuracy |
| Reflection Engine | Missions reflected, lessons extracted, plan improvement rate |
| Skill Engine | Skills loaded, enable/disable rate, permission grant rate |

Metrics feed back into the Learning Engine and are exposed on the
Desktop UI dashboard.

---

## Implementation Sequence

Build in dependency order. Each phase is usable before the next starts.
**Stop writing specs. Start building and measuring.**

```
Phase 1: Foundation (what exists + immediate gaps)
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
├── 🚧 Skill Engine (spec complete → skill-engine.md)
├── 🚧 Tool Selector (spec complete → tool-selection-engine.md)
├── 🚧 Agent Runtime (spec complete → agent-runtime.md)
└── 🚧 Provider Engine (spec complete → provider-engine.md)

Phase 2: Intelligence ← WE ARE HERE
├── 🔨 Cognitive Runtime (spec complete → build first)
├── 🔨 Knowledge Engine (spec complete → build second)
├── 🔨 Evaluation Framework (spec complete → build alongside)
├── Enhanced Memory (6 types)
├── World Model (graph-based)
└── Event namespacing (spec complete → architecture-final.md)

Phase 3: Autonomy
├── Mission Engine (multi-step goals)
├── Goal hierarchy (Goal → Mission → Plan → Execute)
├── Learning Engine (pattern detection)
├── Reflection Engine (post-mission analysis)
├── Automation Engine (proactive behavior)
└── Reasoning Engine (tree search + risk)

Phase 4: Interface
├── Perception Engine (sensors + input)
├── Security Engine (multi-user + biometric)
├── Personality Engine (consistent identity)
├── Conversation Engine (multi-turn + context)
└── Metrics + Observability

Measurement cadence:
├── After every engine: run benchmarks, check metrics
├── Weekly: review trends, identify regressions
├── Monthly: compare against targets, adjust priorities
└── Never: "it feels better" — always "it measures better"
```

## Core Invariants

1. The core contains **no intelligence**. All behavior is in engines/plugins.
2. Nothing is reachable without going through the event bus or transport.
3. Dangerous actions pass through the permission layer before execution.
4. Every new feature answers: **which event does it consume? which does it produce? which engine owns it?**
5. **Skills are data, not code.** Adding a skill = adding a manifest.
6. **The Cognitive Runtime is always running.** FRIDAY is always observing.
7. **Every engine is measurable.** If you can't measure it, you can't improve it.
8. **Memory is experiences. Knowledge is facts.** Never confuse them.
9. **The Agent Runtime owns execution lifecycle.** Not the planner, not the mission engine.
10. **The Provider Engine chooses models. The Tool Selector chooses tools.** Different concerns.
11. **Every engine owns exactly one responsibility.** No God classes. Violation test: if an engine does two things, split it.
12. **Engines talk to the bus, not to each other directly.** Complexity explosion is the enemy.
13. **We build an Adaptive Cognitive OS, not a consciousness simulation.** Optimize for measurable capabilities: planning, adaptation, learning, robustness, situational awareness, autonomy.

## What makes FRIDAY feel like an Adaptive Cognitive OS

| Feeling | Engine |
|---------|--------|
| "It's always thinking" | Cognitive Runtime |
| "It remembers me" | Memory Engine |
| "It knows facts" | Knowledge Engine |
| "It knows what's happening" | World Model |
| "It plans before acting" | Planning + Reasoning |
| "It learns my patterns" | Learning Engine |
| "It acts on its own" | Automation + Mission |
| "It recovers from failures" | Agent Runtime |
| "It reflects on what it did" | Reflection Engine |
| "It has a personality" | Personality Engine |
| "It uses the right tool" | Tool Selector |
| "It picks the right model" | Provider Engine |
| "It stays safe" | Security Engine |
| "It can see and hear" | Perception Engine |

All eighteen engines plus the Cognitive Runtime working together.
That's the operating system for intelligence.

## Conceptual Model: Organs

Internally, the implementation uses `SkillRegistry`, `ToolSelector`, etc.
But conceptually, FRIDAY's capabilities are **organs** — modular systems
that handle specific domains:

```
Vision Organ      → Perception Engine
Voice Organ       → Perception Engine (speech)
Coding Organ      → Skill Engine (rust, python, kotlin)
Research Organ    → Knowledge Engine + Perception Engine
Browser Organ     → Skill Engine (browser)
Developer Organ   → Skill Engine (git, docker, testing)
Minecraft Organ   → Skill Engine (minecraft)
Security Organ    → Security Engine
```

"Organs" is the documentation-level abstraction.
The implementation stays as engines + skills.

## Research Roadmap

For engineering an Adaptive Cognitive OS, study these in order:

### Tier 1 — Directly maps to FRIDAY's architecture

| Month | Focus | Key concepts |
|-------|-------|-------------|
| 1 | **Cognitive Architectures** | Global Workspace Theory, Soar, ACT-R, LIDA, Sigma, OpenCog |
| 2 | **Agent Engineering** | ReAct, Reflexion, Voyager, AutoGen, LangGraph, OpenAI Agents SDK |
| 3 | **Memory Systems + World Models** | Episodic/semantic/procedural memory, causal world models, state graphs |
| 4 | **Knowledge Representation** | RDF, OWL, property graphs, ontologies, Neo4j |
| 5 | **Distributed Systems + Event Architecture** | Actor model, Erlang OTP, Tokio, event sourcing, CQRS |
| 6 | **Human-Computer Interaction** | Calm technology, ambient interfaces, attention management |

### Tier 2 — Valuable patterns

- **Active Inference / Free Energy Principle** — useful for proactive agents
- **Operating Systems** — Linux scheduling, event loops, IPC (transferable patterns)
- **Robotics** — ROS2, behavior trees, sensor fusion (desktop automation ≈ robotics without wheels)

### Tier 3 — Inspiration, not implementation guide

- Consciousness, qualia, philosophy of mind, IIT, panpsychism
- Interesting. Not useful for FRIDAY today.

### Key principle

**Read source code as often as you read papers.** For a project like FRIDAY,
examining mature implementations of agent runtimes, schedulers, databases,
and orchestration frameworks teaches more than theoretical papers.
Theory shows *what* is possible. Code shows *how* to make it reliable.
