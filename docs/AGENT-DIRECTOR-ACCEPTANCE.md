# Agent Director Acceptance Campaign

Agent Director is already implemented as the bounded supervisor above
`AgentRuntime`. This campaign verifies the next multi-agent gate without
introducing a swarm framework or embedding reference projects.

## Contract

```text
Parent Director
  ├── Researcher
  ├── Analyst
  └── Verifier
        ↓
  collect → verify gate → DirectorReport
```

Every child inherits a concurrency limit, deadline, and step budget. A child
must reach `Succeeded`; a verifier must also report at least one successful
check. An unverified child blocks `DirectorReport::all_verified()`.

## Acceptance matrix

| ID | Acceptance | FRIDAY-native evidence | Status |
| --- | --- | --- | --- |
| AD-01 | Parent spawns a bounded child and receives a result | `friday-agent-runtime/tests/director.rs` — `director_runs_the_three_role_topology_and_verifies_every_result` | PASS |
| AD-02 | Researcher + Verifier aggregate through the parent gate | `services/action-runtime/tests/agent_director.rs` — real `SkillAgentDriver`, `DocsAdapter`, and `GitAdapter` | PASS |
| AD-03 | Failed child is visible and can be explicitly reassigned | `failed_child_report_can_be_reassigned_and_verified` | PASS |
| AD-04 | Parent/work item persistence across restart | `services/agent-runtime/tests/workforce.rs` — workforce registry/task/workspace across fresh instances | PASS |
| AD-05 | Child cannot bypass permission/kernel boundary | `services/action-runtime/tests/permission.rs` plus real-driver director acceptance | PASS |
| AD-06 | External-provider work remains behind egress/secrets policy | `services/intelligence` egress/secrets tests; child providers use the same fabric boundary | PASS |
| AD-07 | Unverified child result cannot complete the parent goal | `unverifiable_child_blocks_the_director_run`, verifier-without-check tests, and real-driver gate test | PASS |

## Commands

From the repository root:

```powershell
cargo test -p friday-agent-runtime --test director
cargo test -p friday-agent-runtime --test workforce
cargo test -p friday-actions --test agent_director
cargo test -p friday-actions --test permission
cargo test -p friday-intelligence egress
cargo test -p friday-intelligence secrets
```

The reference projects inform the mechanism only:

- Prime Agent → bounded recursive delegation
- Multica → role registry, task board, and workspaces
- Google ADK → explicit workflow stages

FRIDAY remains the source of truth and keeps children behind its own runtime,
permission, egress, audit, and verification boundaries.

## Boundary

Agent Director is a bounded coordination layer, not generalized autonomy. It
does not create an unlimited swarm, bypass the Action Kernel, or treat child
summaries as truth without verification. Durable child continuation belongs to
the persistent workforce/workflow stores and must be tested separately from
in-memory director topology.
