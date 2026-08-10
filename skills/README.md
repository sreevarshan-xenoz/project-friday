# skills/

The FRIDAY Skill Engine (M5) — modular, installable capabilities that extend the
core brain without rewriting it. **Implementation lives in `services/skills`**
(`friday-skills`); this directory holds the bundled skill manifests.

## Architecture

```
Brain (Core)
  │  Memory · Planner · Personality · Goal Management
  │
  ▼
Skill Engine (services/skills)
  │  SkillLoader · SkillRegistry · SkillContext · SkillHealth · SkillApprovals
  │
  ▼
Capability Intelligence (services/capabilities)
  │  CapabilityRegistry · ServiceOrchestrator · Ranking
  │
  ▼
Services / Adapters
  filesystem · terminal · git · web · docs · codeintel · browser · MCP · ...
```

## What is a Skill?

A Skill is a named, versioned bundle of **pure data**:

- **Capabilities** it needs from the registry
- **Actions** it can perform (natural language)
- **Permissions** it requires (gated by user approval; `readonly` capability
  sets derive `Risk::Safe` automatically)
- **Memory** reads/writes it declares (memory-aware skills)
- **Examples** for few-shot planning
- **Dependencies** on other skills

A Skill is NOT a plugin. It does not contain execution logic, prompts, or
personality. It declares *what* can be done and *how* to plan it; the existing
services do the work. Skills survive prompt rewrites and model swaps by design.

## Directory structure

```
skills/
├── bundled/              Skills shipped with FRIDAY
│   ├── codeintel/        Repository understanding
│   ├── developer/        Code, build, test, deploy
│   ├── docker/           Container management
│   ├── docs/             Documentation search
│   ├── git/              Version control
│   ├── minecraft/        Server management
│   ├── python/           Python scripting
│   ├── rust/             Rust development
│   ├── server/           Remote server management
│   ├── testing/          Test execution
│   └── web/              Web research / browsing
├── user/                 User-installed skills (~/.friday/skills/)
└── README.md
```

## Adding a new skill

The fastest path is the **Skill Forge CLI** (`services/skill-forge`,
`cargo run -p friday-skill-forge --`):

```bash
friday create-skill my-skill --template git
friday validate my-skill
friday publish my-skill
```

Or drop a `skill.json` manifest into `skills/bundled/<id>/` or
`~/.friday/skills/<id>/` by hand:

```json
{
  "id": "my-skill",
  "name": "My Skill",
  "version": "1.0.0",
  "description": "What this skill does.",
  "author": "you",
  "category": "developer",
  "icon": "⚙️",
  "capabilities": ["terminal.execute"],
  "actions": ["do something"],
  "permissions": ["terminal.execute"],
  "memory": {
    "reads": ["my_skill.last_state"],
    "writes": ["my_skill.last_state"]
  },
  "examples": [
    {
      "goal": "Do the thing",
      "plan": ["Step 1", "Step 2"]
    }
  ],
  "dependencies": [],
  "platform": "Any",
  "min_core_version": "0.1.0",
  "enabled": true
}
```

That's it. No code changes. No core modifications.

## Built-in skills

| Skill | Category | What it does |
|-------|----------|-------------|
| `git` | developer | Commit, push, pull, branch, diff, merge, status |
| `developer` | developer | Code, build, test, deploy |
| `docker` | developer | Container management |
| `python` | developer | Python scripting |
| `rust` | developer | Rust development |
| `testing` | developer | Test execution |
| `codeintel` | developer | Repository understanding |
| `docs` | developer | Local documentation search |
| `web` | developer | Web research (soft-declares `web.*`; resolved by `WebAdapter`/`BrowserAdapter`) |
| `minecraft` | gaming | Server management |
| `server` | developer | Remote server management |

## Runtime behavior

- **Skill ranking** fires when `SkillRegistry::context_for()` is called — the
  planner sees ranked candidate skills (with scores), the chosen skill, and its
  resolved capabilities. This feeds the planner prompt's **RELEVANT SKILLS**
  section (explainable selection).
- **Permissions** are declarative: `SkillApprovals` gates capability use; a
  `readonly` capability maps to `Risk::Safe` without special-case code.
- **Health** (`SkillHealth`) reports Healthy / Warning / Broken / Disabled per
  skill so the runtime knows whether a skill *can function now*, not just
  whether it loaded.
- **Qualification** (`qualify()`) validates manifests, resolves dependencies and
  capability ids, and checks platform / core-version constraints at load.

## Status

✅ **M5 shipped.** Loader, registry, ranking, health, approvals, qualification,
bundled skills, and the planner integration are implemented and tested
(`cargo test -p friday-skills`). Skill Forge (M7) scaffolds/validates/publishes
new skills; version *migration* is deliberately deferred (ADR note, not code).
