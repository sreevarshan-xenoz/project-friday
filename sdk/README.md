# sdk/

Cross-language SDK and the canonical protocol schema.

## Today

- **Reference implementation:** the Rust `friday-core` crate (event bus, plugin
  trait, protocol messages, memory interfaces).
- **Mobile wire protocol:** versioned JSON over WebSocket in
  `services/mobile/src/protocol.rs` (`PROTOCOL_VERSION = 1.0`) — treated as a
  public client API for Android and future clients.
- **Windows bridge frames:** versioned `BridgeFrame` codec shared conceptually
  between `plugins/windows` and `windows-host`.
- **MCP:** JSON-RPC client/transports in `friday-mcp`.

## Developer surface

- **Skill manifests** — skills are pure-data `skill.json` bundles (see
  `skills/README.md`); the canonical schema lives in `services/skills`.
- **Skill Forge CLI** (`services/skill-forge`) — `create-skill` / `validate` /
  `publish` for third-party skill authors (M7).

## Planned for this directory

- Published JSON event / protocol schemas exported for non-Rust languages.
- Guidance and stubs for Python / C# / TypeScript plugins over the protocol
  transport.
- Versioning and compatibility notes for multi-language hosts.

See `docs/PluginSDK.md`, `docs/API.md`, `specs/events.md`, `specs/ipc.md`, and
`specs/mobile-console.md`.
