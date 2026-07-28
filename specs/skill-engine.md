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
    "terminal.execute",
    "filesystem.read",
    "filesystem.write"
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
  "permissions": ["terminal.execute", "filesystem.write"],
  "dependencies": [],
  "platform": "Any",
  "min_core_version": "0.1.0",
  "enabled": true
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

- [ ] `friday-skills` builds clean (clippy/fmt/tests)
- [ ] `SkillManifest` parses from `skill.json` files
- [ ] `SkillLoader` discovers skills from bundled + user directories
- [ ] Validation catches: duplicate IDs, missing capabilities, platform mismatch
- [ ] `SkillRegistry` supports enable/disable/deregister at runtime
- [ ] `SkillContext` integrates with `ContextBuilder` (skill data appears in AgentContext)
- [ ] Permission gating works: skill permissions → user approval → planner access
- [ ] Topological sort handles skill dependencies correctly
- [ ] At least 3 built-in skills ship with manifests
- [ ] Adding a new skill requires only a `skill.json` + optional service code — no core changes
- [ ] Skills contain NO prompts, NO instructions, NO personality
- [ ] Tests prove: load → enable → context → permission flow

## 13. What this unlocks

- **User extensibility**: drop in a `skill.json` to teach FRIDAY something new
- **Community marketplace**: share skills across users (future)
- **Better planning**: LLM sees skill capabilities and actions, not raw capability lists
- **Permission clarity**: user approves "Docker skill" once, not every terminal command
- **Composability**: skills can depend on other skills (Docker depends on Developer)
- **Platform awareness**: skills declare platform requirements, only load where compatible
- **Version safety**: skills declare minimum FRIDAY version, preventing incompatible loads
- **Portability**: skills work across LLM providers (no provider-specific prompts)

The brain stays clean. Skills are the interchangeable organs.
