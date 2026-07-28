# skills/

The FRIDAY Skill Engine — modular, installable capabilities that extend the
core brain without rewriting it.

## Architecture

```
Brain (Core)
  │  Memory · Planner · Personality · Goal Management
  │
  ▼
Skill Engine (services/skills)
  │  SkillLoader · SkillRegistry · SkillContext
  │
  ▼
Capability Intelligence (services/capabilities)
  │  CapabilityRegistry · ServiceOrchestrator · Ranking
  │
  ▼
Services / Adapters
  filesystem · terminal · git · windows · MCP · ...
```

## What is a Skill?

A Skill is a named, versioned bundle of:
- **Capabilities** it needs from the registry
- **Actions** it can perform (natural language)
- **Permissions** it requires (gated by user approval)
- **Examples** for few-shot planning
- **Dependencies** on other skills

A Skill is NOT a plugin. It does not contain execution logic. It declares
*what* can be done and *how* to plan it; the existing services do the work.

## Directory structure

```
skills/
├── bundled/              Skills shipped with FRIDAY
│   ├── developer/        Code, build, test, deploy
│   ├── docker/           Container management
│   ├── minecraft/        Server management
│   └── server/           Remote server management
├── user/                 User-installed skills (~/.friday/skills/)
└── README.md
```

## Adding a new skill

Drop a `skill.json` manifest into `skills/bundled/<id>/` or
`~/.friday/skills/<id>/`:

```json
{
  "id": "my-skill",
  "name": "My Skill",
  "version": "1.0.0",
  "description": "What this skill does.",
  "category": "developer",
  "capabilities": ["terminal.execute"],
  "actions": ["do something"],
  "permissions": ["terminal.execute"],
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
| `developer` | developer | Code, build, test, deploy, git |
| `docker` | developer | Container management |
| `minecraft` | gaming | Server management |
| `server` | developer | Remote server management |

## Status

🚧 **Spec complete** (`specs/skill-engine.md`). Implementation pending.
