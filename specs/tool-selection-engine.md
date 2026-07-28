# Tool Selection Engine Specification

**Status:** 🚧 Spec-first — implement against this file.
**Depends on:** `friday-capabilities` (M4.6), `friday-skills` (Skill Engine), `friday-core` (Memory)
**Crate:** `services/tool-selection` — crate `friday-tool-selection`

## 1. Purpose

The LLM should not manually choose tools. When the planner produces a step
like "execute terminal command", the Tool Selection Engine decides *which*
terminal provider, *which* skill context, and *which* permission level to use
— based on history, reliability, cost, and user preference.

This is the feedback loop that makes FRIDAY improve over time:

```
Goal → Plan → Tool Select → Execute → Result → Memory → Better Selection Next Time
```

## 2. The problem it solves

Today, the Capability Registry ranks providers by a static formula:

```
score = reliability * 100 - latency * 0.05 - cost
```

This works for initial selection but doesn't learn. The Tool Selection Engine
adds:

- **Success history** — "Docker terminal succeeded 95% of the time"
- **User preference** — "User always prefers the browser over Windows for URLs"
- **Skill context** — "The Docker skill is active, prefer Docker-aware tools"
- **Permission status** — "User approved terminal for this session"
- **Contextual awareness** — "It's after 5pm, user prefers brief responses"

## 3. Architecture

```
Planner produces action
  │
  ▼
Tool Selector
  │  ├── Query CapabilityRegistry for candidate providers
  │  ├── Query SkillRegistry for active skill context
  │  ├── Query Memory for success history + user preferences
  │  ├── Query Security for permission status
  │  └── Score and rank candidates
  │
  ▼
Best provider selected
  │
  ▼
Executor runs action via selected provider
  │
  ▼
Result fed back:
  ├── Memory: update success/failure counts
  ├── CapabilityRegistry: update reliability scores
  └── ToolSelector: update preference weights
```

## 4. Selection model

### 4.1 Candidate scoring

Each candidate provider is scored on multiple dimensions:

```rust
pub struct SelectionScore {
    pub reliability: f64,      // historical success rate (0..1)
    pub latency: f64,          // normalized latency penalty (0..1)
    pub user_preference: f64,  // how often user chose this (0..1)
    pub skill_context: f64,    // bonus if active skill uses this provider (0..1)
    pub permission_status: f64, // 1.0 if approved, 0.0 if needs prompting
    pub platform_match: f64,   // 1.0 if compatible, 0.0 if not
    pub cost: f64,             // normalized cost penalty (0..1)
}
```

### 4.2 Final score

```
final = (reliability * 0.35)
      + ((1 - latency) * 0.15)
      + (user_preference * 0.20)
      + (skill_context * 0.15)
      + (permission_status * 0.10)
      + (platform_match * 0.05)
      - (cost * 0.05)
```

Weights are tunable and stored in config. The key difference from the
existing registry ranking: **user_preference** and **skill_context** carry
significant weight (35% combined), making FRIDAY adapt to the user.

### 4.3 Tie-breaking

When scores are close (within 0.05):
1. Prefer the provider with more total executions (experience wins)
2. Prefer the provider the user most recently used (recency wins)
3. Prefer lower latency (speed wins)

## 5. Memory integration

The Tool Selection Engine reads and writes to Memory:

### 5.1 Reading

- **Success history**: for each (provider, action) pair, how many times
  succeeded vs failed
- **User preference**: when multiple providers were available, which did
  the user accept/reject?
- **Recency**: when was the last successful execution?

### 5.2 Writing

After every execution, the Tool Selector records:

```rust
pub struct SelectionRecord {
    pub goal: String,
    pub action: String,
    pub selected_provider: String,
    pub candidate_count: usize,
    pub score_at_selection: f64,
    pub success: bool,
    pub latency_ms: u64,
    pub timestamp: DateTime<Utc>,
}
```

This creates a feedback loop: the more FRIDAY executes, the better it
gets at choosing tools.

## 6. Skill context integration

When a skill is active (e.g., `docker` skill), the Tool Selector gives
bonus score to providers that skill declares:

```
Goal: "build the docker image"
Active skill: docker (declares: terminal.execute)
Candidates: [local-terminal, remote-terminal, browser]
Scoring: local-terminal gets +0.15 skill_context bonus
Result: local-terminal selected (even if remote has slightly better latency)
```

Without an active skill, skill_context is 0 for all candidates (neutral).

## 7. Permission integration

The Tool Selector checks permission status before scoring:

- If a provider requires permissions the user has already approved →
  `permission_status = 1.0`
- If a provider requires permissions that need prompting →
  `permission_status = 0.3` (penalized but not eliminated)
- If a provider is blocked by policy →
  `permission_status = 0.0` (eliminated)

This means FRIDAY prefers tools it already has permission to use, but
can still suggest new tools if they score well enough.

## 8. Events

| Event | Payload | Producer |
|-------|---------|----------|
| `tool.selected` | `{ goal, action, provider, score, candidates }` | ToolSelector |
| `tool.executed` | `{ provider, action, success, latency_ms }` | ToolSelector |
| `tool.ranked` | `{ action, rankings: Vec<(provider, score)> }` | ToolSelector |
| `tool.preference` | `{ provider, action, accepted: bool }` | ToolSelector |

## 9. Acceptance criteria

- [ ] `friday-tool-selection` builds clean (clippy/fmt/tests)
- [ ] Candidate scoring produces deterministic rankings
- [ ] Memory integration: success history updates after each execution
- [ ] User preference learning: accepted/rejected tools affect future ranking
- [ ] Skill context bonus: active skill providers rank higher
- [ ] Permission awareness: approved tools preferred over unapproved
- [ ] Tie-breaking: recency + experience + latency as fallbacks
- [ ] Weights are configurable (not hardcoded)
- [ ] Tests prove: cold start → execution → memory update → improved selection
- [ ] Integration test: same goal produces better tool selection after 10 runs

## 10. What this unlocks

- **Self-improvement**: FRIDAY gets better at choosing tools over time
- **User adaptation**: learns which tools the user prefers
- **Skill awareness**: active skill influences tool selection
- **Permission efficiency**: reuses approved permissions, minimizes prompts
- **Graceful degradation**: falls back to registry ranking when memory is empty
- **Debuggability**: every selection is recorded with reasoning (score breakdown)
