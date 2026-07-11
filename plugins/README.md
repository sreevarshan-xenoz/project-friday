# plugins/

Individual capability plugins (Voice, Vision, Browser, Terminal, Git, Windows,
Discord, Spotify, Steam, OBS, VSCode, …). Each is a separate crate/package that
implements the `Plugin` trait from `core/src/plugin/sdk.rs`.

The reference plugin is `core/examples/echo_plugin.rs`. See `docs/PluginSDK.md`.
