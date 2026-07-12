# Recovery Policy

Recovery is now **infrastructure**. Treat it as boring and stable.

## Policy on changing Recovery

- Acceptable: bug fixes, performance, benchmark improvements.
- **Not** acceptable: redesign, new architectural shape, new abstractions bolted
  onto the current action-level graph.
- The frozen surface includes: the four resilience metrics, the `resolves()`
  contract, and the `RecoveryMode`/lifecycle. Changing any of these is a redesign.

## Action-level vs state-level recovery (separation)

- **Action-level (current, frozen)**: `friday-recovery` maps a `FailureCategory`
  to ordered `RecoveryAction` steps in a `RecoveryGraph`. This is the only recovery
  abstraction that exists today and it is frozen.
- **State-level (future, separate)**: repository-state and filesystem-state
  machines (see below). These MUST be built as a *distinct* abstraction and must
  NOT be merged into the action-level graph. The two layers may coexist — action-level
  handles transient/provider failures; state-level handles repository/filesystem
  state — but the boundary between them must stay explicit so each evolves
  independently.

## Honest resolution rules

`RecoveryAction::resolves(category, available_caps)` is the contract that keeps
the benchmark honest:

- Reroute steps (`UseProvider`, `UseAlternateCapability`) resolve **iff** their
  target capability is available.
- Conditional steps (`Retry`, `RestartService`, …) resolve **only** for transient
  categories (`Network` / `Timeout` / `Resource`). A dead provider is *not* healed
  by retrying it.
- Escalation steps (`AskUser`, `Escalate`, `SuggestElevation`) never auto-resolve.

## Runtime

Until wired into `AgentKernel`, the graph runs in **shadow mode only** (record
"I would have chosen…" without acting). Live recovery is enabled only after shadow
agreement, following the `Prototype → Shadow → Limited → Production → Qualified`
ladder in `qualification.md`.

## Future direction (NOT now)

When genuine recovery for `git` and `filesystem` is eventually built, make it
**state-driven**, as a separate state-level abstraction (above):

- **Repository Recovery** instead of "Git recovery": a repository-state machine
  (`Clean`, `Dirty`, `Detached HEAD`, `Conflict`, `Rebase`, `Cherry Pick`,
  `Bisect`, `Locked`, `Corrupt`) → Recovery Strategy. This survives swapping the
  Git CLI for `libgit2` or Jujutsu.
- **Filesystem state** recovery: `Permission → Elevation`,
  `Disk Full → Cleanup → Retry`, `Read Only → Remount → Retry`.

Recovery should become state-driven whenever possible.
