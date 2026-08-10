# tests/

Cross-crate integration and system-test notes. Most automated tests live **next
to the crates they cover** (Rust convention).

## Where tests actually are

| Suite | Location |
| ----- | -------- |
| Core bus / plugins / memory | `core/tests/` |
| Planner pipeline + MCP integration | `planner/tests/` |
| AgentKernel / adapters / kernel | `services/action-runtime/tests/` |
| Capabilities / orchestrator | `services/capabilities/tests/` |
| Simulation | `services/simulation/tests/` |
| Skill Engine | `services/skills/tests/` + `services/skill-forge/tests/` |
| Goal Execution (evidence, workflows, admission, executive) | `services/goal-execution/tests/` |
| Investigation / Agent Runtime / Persona / Knowledge / Project Cognition | `services/*/tests/` |
| Mobile protocol / transport / auth | `services/mobile/` (in-crate tests) |
| ReadModelBridge | `services/bridge/tests/` |
| Context / intelligence / recovery | `services/*/tests/` |
| MCP transports | `mcp/tests/` |
| Windows bridge | `plugins/windows/tests/` |
| End-to-end harness (incl. live local-model + skill-planning suites) | `e2e/` (`friday-e2e`) |
| Qualification evidence (not unit tests) | `benchmarks/` |

```powershell
cargo test -p friday-core
cargo test -p friday-planner
cargo test -p friday-actions
cargo test -p friday-mcp
# broader workspace (can take a while):
cargo test --workspace
```

Real Windows host tests are opt-in (`FRIDAY_WINDOWS_HOST_TEST`); CI uses mocks.
