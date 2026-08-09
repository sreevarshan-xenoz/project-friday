# Skill Engine Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-capabilities` (M4.6), `friday-planner` (M3), `friday-context` (M4.10)
**Crate:** `services/skills` — crate `friday-skills`

## 1. Purpose

FRIDAY should not hardcode hundreds of abilities. It should gain new skills
without rewriting the core. The Skill Engine is the layer that makes this
possible: it wraps the existing Capability Intelligence system into a
user-facing, installable, composable skill model.

The existing architecture already has:
- **Capability** — atomic action (`terminal.execute`, `filesystem.create_file`)
- **CapabilityRegistry** — ranked providers for each capability
- **ServiceManifest** — disk-based discovery of service capabilities
- **ServiceOrchestrator** — lifecycle and health management
- **ContextBuilder** — assembles context for the LLM

The Skill Engine adds:
- **Skill** — composed set of capabilities + metadata (pure data, no prompts)
- **SkillLoader** — discovery, loading, validation, versioning
- **SkillContext** — skill-aware context for the planner

### 1.1 Design rule: Skills are pure data

Skills do NOT contain prompts, instructions, or personality. They are
declarations of what a skill *can do*, not how to do it. The Context Builder
constructs the LLM prompt using skill data as input, but the skill itself
never contains prompt text.

This separation ensures:
- Skills are portable across LLM providers
- Skills don't break when the prompt template changes
- Skills are testable without an LLM
- Users can create skills without writing prompts
- The prompt strategy lives in one place (Context Builder)

## 2. Relationship to existing architecture

```
USER
  │
  ▼
Brain (Core)
  │  Memory · Planner · Personality · Goal Management
  │
  ▼
Skill Engine (friday-skills)
  │  SkillLoader · SkillRegistry · SkillContext
  │
  ▼
Capability Intelligence (friday-capabilities)
  │  CapabilityRegistry · ServiceOrchestrator · Ranking
  │
  ▼
Services / Adapters
  │  filesystem · terminal · git · windows · MCP · ...
  │
  ▼
Devices
   Windows PC · Android Phone · Browser · Smart Home · ...
```

The Skill Engine sits **between** the Planner and the Capability Registry. It
does not replace either — it enriches the planner's view with skill-level
metadata and gives users a simple way to extend FRIDAY.

## 3. Skill model

A **Skill** is a named, versioned declaration of capabilities and metadata.
It is purely data — no prompts, no instructions, no personality.

### 3.1 `SkillManifest` (on-disk)

```json
{
  "id": "docker",
  "name": "Docker",
  "version": "1.0.0",
  "description": "Build, run, and manage Docker containers and images.",
  "author": "FRIDAY",
  "category": "developer",
  "icon": "docker",
  "capabilities": [
    "terminal.exec",
    "filesystem.read_file",
    "filesystem.write_file"
  ],
  "actions": [
    "build docker image",
    "run docker container",
    "list docker containers",
    "stop docker container",
    "docker compose up",
    "docker compose down",
    "view docker logs"
  ],
  "permissions": ["terminal.exec", "filesystem.write_file"],
  "dependencies": [],
  "platform": "Any",
  "min_core_version": "0.1.0",
  "enabled": true,
  "memory": {
    "reads": ["docker.last_image"],
    "writes": ["docker.last_image"]
  }
}
```

### 3.2 `Skill` (runtime)

```rust
pub struct Skill {
    pub manifest: SkillManifest,
    pub capabilities: Vec<Capability>,  // resolved from registry
    pub loaded_at: DateTime<Utc>,
    pub enabled: bool,
}
```

### 3.3 Fields

| Field | Meaning |
|-------|---------|
| `id` | Unique slug (`docker`, `camera`, `smart-home`) |
| `name` | Human-readable name |
| `version` | SemVer |
| `description` | What this skill does (one sentence) |
| `category` | Grouping: `developer`, `productivity`, `media`, `iot`, `gaming`, `security` |
| `capabilities` | Capability IDs this skill needs from the registry |
| `actions` | Natural-language action descriptions (used for goal matching, not prompts) |
| `permissions` | Permission strings required (gated by user approval) |
| `dependencies` | Other skill IDs required |
| `platform` | Target platform (`Windows`, `Any`, etc.) |
| `min_core_version` | Minimum FRIDAY version required |
| `enabled` | Whether this skill is active |
| `memory` (§14) | Declared memory keys: `reads` (keyed recall) + `writes` (persisted facts) |

### 3.4 What skills do NOT contain

Skills do NOT contain:
- **System prompts** — belong in Context Builder
- **Instructions** — belong in Context Builder
- **Personality** — belongs in `personality.rs`
- **Examples** — belong in project documentation or context
- **Templates** — belong in Context Builder
- **Tone/style guidance** — belongs in `personality.rs`

The Context Builder reads skill metadata and constructs appropriate
prompt sections. The skill is a data source, not a prompt source.

## 4. SkillLoader

Discovers, validates, and loads skills from multiple sources.

### 4.1 Sources

| Source | Path | Use case |
|--------|------|----------|
| Built-in | `skills/bundled/` | Core skills shipped with FRIDAY |
| User | `~/.friday/skills/` | User-installed skills |
| Plugin | `plugins/*/skill.json` | Service-attached skills |
| Remote (future) | Registry API | Community skill marketplace |

### 4.2 Loading pipeline

```
scan directories
  → parse skill.json
  → validate (id unique, capabilities exist, platform compatible)
  → check dependencies (all deps loaded?)
  → check min_core_version
  → resolve capabilities from CapabilityRegistry
  → register into SkillRegistry
  → emit skill.loaded event
```

### 4.3 Validation rules

- `id` must be unique across all loaded skills
- All `capabilities` must exist in the CapabilityRegistry (or be soft-declared
  for future capabilities)
- `platform` must be compatible with current host
- `min_core_version` must be satisfied
- `dependencies` must be loaded first (topological sort)
- `actions` must be non-empty (at least one action description)
- `skill.json` must be valid JSON matching the SkillManifest schema

## 5. SkillRegistry

In-memory registry of loaded skills. The planner's view of available skills.

### 5.1 Operations

```rust
impl SkillRegistry {
    fn register(skill: Skill) -> Result<()>;
    fn deregister(skill_id: &str) -> Result<()>;
    fn enable(skill_id: &str) -> Result<()>;
    fn disable(skill_id: &str) -> Result<()>;
    fn get(skill_id: &str) -> Option<&Skill>;
    fn all() -> &[Skill];
    fn enabled() -> Vec<&Skill>;
    fn for_action(action: &str) -> Vec<&Skill>;  // skills whose actions match
    fn for_capability(cap_id: &str) -> Vec<&Skill>;  // skills that need this cap
    fn categories() -> Vec<String>;
    fn by_category(category: &str) -> Vec<&Skill>;
}
```

### 5.2 Skill ranking

When the planner receives a goal, the Skill Engine suggests relevant skills:

```
goal: "build the docker image"
  → skill_search(goal) → [docker: 0.95, developer: 0.72]
  → docker skill provides: action hints, permission hints, capability requirements
  → planner uses this to build a better plan
```

Ranking uses:
1. **Action match** — how many skill actions match the goal (BM25 or cosine)
2. **Capability availability** — skills whose capabilities are all healthy rank higher
3. **User preference** — skills the user has used recently rank higher

## 6. SkillContext

The skill-aware context handed to the Context Builder. Supplements `AgentContext`
with skill-level metadata. Pure data — no prompts.

```rust
pub struct SkillContext {
    pub relevant_skills: Vec<SkillSummary>,
    pub active_skill: Option<String>,  // currently active skill if any
}

pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub actions: Vec<String>,
    pub capabilities: Vec<String>,
    pub permission_hints: Vec<String>,
}
```

The Context Builder calls `SkillRegistry::for_action(goal)` to find relevant
skills, extracts their summaries, and includes them in `AgentContext`. The
Context Builder then constructs appropriate prompt sections from this data.

The LLM sees:
```
Goal: "build the docker image"

Relevant skills:
  - Docker (v1.0.0): Build, run, and manage Docker containers
    Actions: build docker image, run docker container, ...
    Capabilities needed: terminal.execute, filesystem.read
    Permissions: terminal.execute, filesystem.write
```

The Context Builder decides how to present this information. The skill
just provides the data.

### 6.1 Plan-time view (PlanContext)

The same `SkillContext` also reaches the **LLM planner's prompt** — the
plan-time view, distinct from the Context Builder's run-time view. The boot
path (`friday_context::register_engines_with_skills` →
`friday_planner::register_engines_with_policy`) hands the loaded
`SkillRegistry` to the planning engine as a `SkillContextSource`; every
`PlanContext` then carries the skills ranked against the goal
(`PlanContext.skills: Option<SkillContext>`), and `LlmPlanner::build_prompt`
renders them as a `RELEVANT SKILLS` section between the goal and the
capability list:

```
GOAL: "build the docker image"

RELEVANT SKILLS (ranked for this goal; active_skill is the recommended
skill — plan through it when it fits, using the capabilities it declares):
{
  "relevant_skills": [{ "id": "docker", "version": "1.0.0", ... }],
  "active_skill": "docker"
}

AVAILABLE CAPABILITIES ...
```

Design rules: the section is pure data (no prompts, no scores — rank is
encoded by ordering), advisory like experience evidence (a broken skill setup
yields `None`, never a stranded goal), the rule planner ignores it entirely,
and the ranking pass runs **only for LLM-routed goals** (a rule-planned goal
never pays for a skill ranking it will ignore). Trust surface: skill manifest
text (name/description/actions) reaches the planning model — JSON-escaped but
still readable, the same trust class as capability descriptions; since skills
are third-party-installable via Skill Forge, a skill author controls text the
planner will read (documented, not fixed — consistent with the house pattern).
Proven by `llm_planner_prompt_includes_ranked_bundled_skills` in
`services/context/tests/skill_runtime.rs`, which records the actual prompt
handed to the model and asserts the bundled `git` skill is ranked in it.

## 7. Permission model

Skills declare `permissions` — the capabilities they need user approval for.
The Skill Engine integrates with the existing permission layer:

1. Skill declares `permissions: ["terminal.execute", "filesystem.write"]`
2. User approves the skill (once, per-session, or per-action)
3. Approval is cached in the session
4. Planner sees approved permissions and can use them freely
5. New permissions trigger a user prompt

This maps to the existing `PolicyAuthority` trait in the planner. The
SkillEngine becomes a policy source that tracks skill-level approvals.

## 8. Skill lifecycle

```
              ┌──────────┐
              │ Scanned   │  manifest found on disk
              └────┬─────┘
                   │ validate
              ┌────▼─────┐
              │ Loaded    │  manifest valid, capabilities resolved
              └────┬─────┘
                   │ enable
              ┌────▼─────┐
              │ Enabled   │  active, available to planner
              └────┬─────┘
                   │ disable / error
              ┌────▼─────┐
              │ Disabled  │  temporarily off
              └────┬─────┘
                   │ uninstall
              ┌────▼─────┐
              │ Removed   │  deleted from disk
              └──────────┘
```

Events emitted: `skill.scanned`, `skill.loaded`, `skill.enabled`, `skill.disabled`,
`skill.removed`, `skill.error`.

## 9. Built-in skills (initial set)

Skills are **granular** — one skill per tool, not one skill per domain.
This enables fine-grained install/upgrade/disable/share.

| Skill | Category | Capabilities | Notes |
|-------|----------|-------------|-------|
| `git` | developer | git.status, git.commit, git.push, git.pull, git.diff | Version control |
| `rust` | developer | terminal.execute, filesystem.* | Cargo, rustc, clippy |
| `python` | developer | terminal.execute, filesystem.* | pip, pytest, venv |
| `kotlin` | developer | terminal.execute, filesystem.* | Gradle, Android builds |
| `docker` | developer | terminal.execute, filesystem.* | Containers, Compose |
| `testing` | developer | terminal.execute | Test runners across languages |
| `deployment` | developer | terminal.execute, filesystem.* | CI/CD, release pipelines |
| `benchmarking` | developer | terminal.execute | Performance measurement |
| `browser` | productivity | browser.* (M6) | Web navigation, research |
| `camera` | media | vision.* (M6.4) | Camera, screenshots, OCR |
| `voice` | media | voice.* (M6.3) | Speech recognition, TTS |
| `smart-home` | iot | mqtt, http | Lights, sensors, devices |
| `minecraft` | gaming | terminal.execute, rcon | Server management |
| `server` | developer | terminal.execute, ssh | Remote server management |
| `calendar` | productivity | calendar.* | Scheduling, reminders |
| `email` | productivity | email.* | Read, send, organize |

Users can add new skills by dropping a `skill.json` manifest into
`~/.friday/skills/<id>/`. Granular skills mean users can:

- `Install Rust Skill` — adds Rust support without Docker
- `Disable Kubernetes Skill` — turn off what you don't use
- `Upgrade Docker Skill` — update one skill, not all of developer
- `Share Git Skill` — publish your custom git workflow

## 10. Remote skill registry (future)

A community skill marketplace where users can:
- Browse available skills
- Install with one click
- Rate and review
- Publish their own skills

This is a future extension. The local Skill Engine must work completely
offline — remote registry is optional enhancement.

## 11. Event vocabulary

| Event | Payload | Producer |
|-------|---------|----------|
| `skill.scanned` | `{ source, count }` | SkillLoader |
| `skill.loaded` | `{ skill_id, version, capabilities }` | SkillLoader |
| `skill.enabled` | `{ skill_id }` | SkillRegistry |
| `skill.disabled` | `{ skill_id }` | SkillRegistry |
| `skill.removed` | `{ skill_id }` | SkillLoader |
| `skill.error` | `{ skill_id, error }` | SkillLoader |
| `skill.activated` | `{ skill_id, goal }` | SkillContext |
| `skill.deactivated` | `{ skill_id }` | SkillContext |

## 12. Acceptance criteria

- [x] `friday-skills` builds clean (clippy/fmt/tests)
- [x] `SkillManifest` parses from `skill.json` files
- [x] `SkillLoader` discovers skills from bundled + user directories
- [x] Validation catches: duplicate IDs, missing capabilities, platform mismatch
- [x] `SkillRegistry` supports enable/disable/deregister at runtime
- [x] `SkillContext` integrates with `ContextBuilder` (skill data appears in AgentContext)
- [x] Permission gating works: skill permissions → user approval → planner access
- [x] Topological sort handles skill dependencies correctly
- [x] At least 3 built-in skills ship with manifests (8: git, docker, rust, python, testing, server, minecraft, developer)
- [x] Adding a new skill requires only a `skill.json` + optional service code — no core changes
- [x] Skills contain NO prompts, NO instructions, NO personality
- [x] Tests prove: load → enable → context → permission flow
- [x] **Runtime wiring** (2026-08-08): `friday_context::register_engines_with_skills` boots the planner stack with the skill-aware `SkillPolicyAuthority` as the supervisor policy, and `SkillRegistry::context_for` emits an auditable per-goal ranking trace (`skill ranking: candidate skills scored against goal`). Proven by `services/context/tests/skill_runtime.rs` against the real native registry: 8/8 bundled manifests load/resolve/qualify, medium-risk skill-covered capabilities pause until approval then resume to completion, uncovered capabilities defer to allow-all, High-risk capabilities pause even when approved, and `with_skills` surfaces bundled skills in `AgentContext`.
- [x] **M5 battle-tested** (2026-08-08): skill health view (`SkillHealth`), live capability re-resolution (`reconcile`), deterministic chosen-skill selection (`select_for` + `[Skill Decision]` trace), and the adversarial behavior suite — all in `services/context/tests/skill_runtime.rs` (11 tests) + `friday-skills` (38 unit + 6 integration).
- [x] **M7 Skill Forge CLI** (2026-08-08): `friday create-skill / validate / publish` in `services/skill-forge` (binary `friday`); templates scaffold canonical manifests; `validate` qualifies against the real native registry; `publish` installs into `~/.friday/skills` and refuses broken or overwriting installs. 7 integration tests + a live CLI smoke run.
- [x] **M8 memory-aware skills** (2026-08-08): `SkillManifest.memory` (`reads`/`writes`) + gated per-skill `SkillMemory` handle (`friday_skills::memory`), `SkillEngineWiring::with_memory/memory_for`, `friday_context::CoreMemorySource` over the real `MemoryEngine` (LongTerm store / LongTerm+Session search). Proven end-to-end: the bundled `git` skill stores + reads back `git.last_workspace` through a real redb-backed engine, and undeclared keys are rejected at the gate.
- [x] **Plan-time skill view** (2026-08-08): `PlanContext.skills: Option<SkillContext>` populated by the planning engine's skill source (`PlanningEngine::with_skill_context`); the LLM planner's prompt renders a `RELEVANT SKILLS` section (ranked summaries + `active_skill`) between the goal and the capability list (§6.1). The policy factory in `register_engines_with_policy` now returns a `PolicyBinding { policy, skills }` so the same loaded registry feeds both the supervisor's permission policy and the planner's context. Proven by the recording-backend e2e test (`llm_planner_prompt_includes_ranked_bundled_skills`) asserting the actual model prompt carries the ranked `git` skill.

## 13. Skill Forge (M7)

Skill Forge turns FRIDAY into a platform: third parties create, validate, and
publish skills without touching the core. The CLI lives in
`services/skill-forge` (crate `friday-skill-forge`, binary **`friday`**); the
command logic is a library so it is testable without spawning the binary.

### 13.1 Commands

| Command | Behavior |
|---------|----------|
| `friday create-skill <id> [--template git|docker|minecraft|server|python|generic] [--dir <path>]` | Scaffolds `<dir>/<id>/skill.json` + `README.md` from a template. Templates use **canonical** capability ids, so a fresh skill validates on the first run. `id` must be a slug (`a-z 0-9 - _`). |
| `friday validate [<path>]` | Loads every skill under `<path>` (default `.`) with the real loader, resolves against the **native registry** (`friday_planner::execution::native_caps`), runs `qualify()` per skill, and prints a per-check report. Exit 1 on any load failure or failed check (or when **no skills** are found — validating nothing is not a green pass). Limitation: `validate` only knows native capabilities today, so a skill targeting a future MCP/plugin capability (`mcp:*`, `browser.*`) soft-declares it and shows a failed `capabilities` check; a runtime-provided extra-capability list is future work. |
| `friday publish <id-or-path> [--dest <path>] [--force]` | Validates first (a broken skill is **never** installed), then copies the skill into `~/.friday/skills/<id>/` (HOME/USERPROFILE aware; `--dest` overrides). Refuses to overwrite without `--force`. |
| `friday help` / `friday version` | Usage / version. |

### 13.2 Packaging

A published skill is the same directory structure the loader already reads
(`<id>/skill.json`, optionally `README.md`, `examples/`, ...), so publishing
is just a validated copy — no new on-disk format, no registry server. A
remote marketplace (§10) can later publish to a server instead; the local
flow stays offline.

## 14. Memory-aware skills (M8)

Skills stay pure data, but they may *declare* deterministic memory instead of
asking the model to "remember":

```json
"memory": { "reads": ["minecraft.server_state"], "writes": ["minecraft.server_state"] }
```

At runtime each loaded skill can be handed a **gated** handle
(`friday_skills::memory::SkillMemory`) over an abstract
`SkillMemorySource` (`search` / `read` / `store`):

| Operation | Allowed for | Notes |
|-----------|-------------|-------|
| `search(query, limit)` | scoped to keys in `memory.reads` | free-text recall across LongTerm + Session, filtered to declared read keys — a skill can only ever retrieve content under keys it declared (no cross-skill exfiltration by searching) |
| `read(key)` | keys declared in `memory.reads` | deterministic keyed recall; `Ok(None)` = never stored |
| `store(key, value)` | keys declared in `memory.writes` | durable LongTerm persistence; disabled skills are denied everything |

Enforcement lives in the gate (never in the store): a skill cannot read or
write a key it did not declare, cannot search beyond its declared read keys,
and a disabled skill is permission-inert. The gate reads the skill's **live**
registry state per operation — disabling a skill mid-session revokes an
already-issued handle. The real store is wired at the integration point:
`friday_context::CoreMemorySource` wraps the runtime `MemoryEngine`, and
`SkillEngineWiring::with_memory(source)` + `SkillEngineWiring::memory_for(id)`
expose the handles. Proven live: the bundled `git` skill stored and read back
`git.last_workspace` through a redb-backed `MemoryEngine`.

## 15. What this unlocks

- **User extensibility**: drop in a `skill.json` to teach FRIDAY something new
- **Community marketplace**: share skills across users (future)
- **Better planning**: LLM sees skill capabilities and actions, not raw capability lists
- **Permission clarity**: user approves "Docker skill" once, not every terminal command
- **Composability**: skills can depend on other skills (Docker depends on Developer)
- **Platform awareness**: skills declare platform requirements, only load where compatible
- **Version safety**: skills declare minimum FRIDAY version, preventing incompatible loads
- **Portability**: skills work across LLM providers (no provider-specific prompts)

The brain stays clean. Skills are the interchangeable organs.

---

# §15 Agent scoping & the self-development skills

## 15.1 `SkillRegistry::subset` — per-agent allowed skills

The Agent Runtime gives each worker an **allowed-skills scope**. A worker
physically cannot reach a skill outside its scope:

```rust
let scoped: SkillRegistry = registry.subset(&["git"]); // + git's dependency closure
```

- Empty allowed list = every loaded skill.
- Dependency closure rides along (a skill whose dependency is out of scope is
  dropped — it would be unselectable anyway).
- Enabled/disabled state is preserved from the source registry.
- The subset is a point-in-time snapshot per worker — workers never mutate the
  shared registry.

`select_for` + `SkillPolicy` over the subset make the scoping structural: an
agent scoped to `[git]` gets git's capabilities, no others, even if the goal
mentions documentation.

## 15.2 Bundled self-development skills

The "what does FRIDAY need to build the next version of itself" shortlist is
now bundled (`skills/bundled/`), all readonly (safe capabilities auto-approve):

| Skill | Capabilities | Notes |
|-------|--------------|-------|
| `git` | `git.*`, `terminal.exec` | already bundled |
| `web` | `web.search`, `web.fetch` + **soft-declared** `web.open/click/type/scroll/extract/screenshot` | HTTP read-only v1; browser verbs bind when a browser runtime arrives |
| `docs` | `docs.search`, `docs.read` | search/read local docs (READMEs, ADRs, specs) |
| `codeintel` | `codeintel.understand` | repo structure/counts/query matches |

The soft-declared browser verbs exercise the M5 soft-declaration mechanism for
real: the `web` skill is selectable today (its read-only verbs resolve) and
automatically gains the browser verbs the moment a future adapter registers
them.
