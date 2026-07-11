# Services Specification

A **service** (not a bare "plugin") is the organizing unit above the `ActionAdapter`
trait. A service may internally use multiple adapters/bridges (e.g. the Windows
service = Rust adapter + C# host over IPC).

## Crate / home

`services/action-runtime/src/adapters/` — `ActionAdapter` trait. Each service
implements `ActionAdapter` (`tool` name) and exposes `capabilities()`.

## `ActionAdapter` (unchanged, locked)

```rust
trait ActionAdapter {
    fn tool(&self) -> &str;
    async fn run_action(&self, action: ActionRequest) -> Result<ActionResult, ActionError>;
    fn capabilities(&self) -> Vec<Capability> { .. }  // default empty
}
```

`AgentKernel::run_action` is the single dispatch path; it maps `action.tool` to the
registered adapter.

## Service inventory

| Service    | `tool`     | Crate / module                | Notes |
|------------|------------|-------------------------------|-------|
| Filesystem | `fs`       | `adapters::filesystem`        | ✅ M4.5 |
| Terminal   | `terminal` | `adapters::terminal`          | ✅ M4.4 |
| Git        | `git`      | `adapters::git` (`friday-actions`) | ✅ M4.5.1 |
| Windows    | `windows`  | `plugins/windows` (`friday-windows`) | ✅ M4.5.2, IPC |
| Browser    | `browser`  | (future, M6)                  | declares capabilities only |

## Capability publication

Every service publishes a `Vec<Capability>` (via `capabilities()` or
`manifest.json`) into `CapabilityRegistry`. `adapters::all_capabilities()` aggregates
the in-code ones; `ServiceManifest` scans for manifests.

## Simulatable (M4.7 requirement)

To support the Simulation Engine, each service adapter **also** implements a
`simulate` path (see `specs/simulation.md`) returning virtual `SimulatedEffect`s.
Providers whose effects cannot be predicted safely set `Capability.dry_run = false`
and are skipped by the Simulator.

## See also

- `PluginSDK.md` (locked) for the adapter contract.
- `services.md` in `docs/Architecture.md` addendum.
