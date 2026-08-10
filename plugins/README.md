# plugins/

Capability plugins that implement the `Plugin` trait from
`core/src/plugin/sdk.rs`, or platform bridges that sit next to the AgentKernel
adapters.

## Shipped

| Plugin / crate | Path | Status | Notes |
| -------------- | ---- | ------ | ----- |
| Echo (reference) | `core/examples/echo_plugin.rs` | ✅ | `demo.ping` → `demo.pong` |
| Windows bridge client | `plugins/windows` (`friday-windows`) | ✅ | Named pipes → `windows-host` C#; intent-level `windows.*` actions |

## Delivered as services / adapters (not separate plugin crates)

These are `ActionAdapter`s under `services/action-runtime`, registered with the
**AgentKernel**:

| Capability | Adapter | Status |
| ---------- | ------- | ------ |
| Filesystem | `FilesystemAdapter` | ✅ |
| Terminal | `TerminalAdapter` | ✅ |
| Git | `GitAdapter` | ✅ |
| Web (HTTP fetch + search) | `WebAdapter` | ✅ |
| Docs | `DocsAdapter` | ✅ |
| Code Intelligence | `CodeIntelAdapter` | ✅ |
| Browser (CDP headless Chrome) | `BrowserAdapter` | ✅ (e2e-wired) |
| MCP tools | `McpAdapter` | ✅ |
| Simulation | `SimAdapter` | ✅ |

## Planned (not yet crates here)

Voice, Vision, Discord, Spotify, Steam, OBS, VSCode, and other third-party
integrations. Event kinds may already be reserved in `docs/Modules.md` /
`docs/Voice.md` / `docs/VisionSystem.md`. (A Browser runtime now exists as the
`BrowserAdapter` above; deeper browser-organs remain open.)

## Adding a plugin

1. Create a crate under `plugins/<name>/` (or a service under `services/`).
2. Implement `Plugin` and/or `ActionAdapter` as appropriate.
3. Declare `subscribes` / `emits` (or capability manifest).
4. Register with `Friday::register_plugin` and/or `AgentKernel`.
5. Document in `docs/Modules.md` and ship a `specs/` entry if it is a subsystem.

See `docs/PluginSDK.md` and `specs/services.md`.
