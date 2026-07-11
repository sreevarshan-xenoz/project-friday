# Modules

FRIDAY itself knows almost nothing. Every capability is a **plugin**. If a better
implementation appears, you swap the plugin — you do not rewrite the application.

## Plugin lifecycle

Defined by the `Plugin` trait (`core/src/plugin/sdk.rs`):

1. `metadata()` — static declaration of identity and interests.
2. `init(&ctx)` — acquire resources, called once before routing starts.
3. `handle_event(&event) -> Vec<Event>` — do the work, emit follow-up events.
4. `shutdown()` — release resources and flush state.

## Planned capabilities

| Module / Plugin     | Kind(s) it subscribes to      | Language | Status     |
| ------------------- | ----------------------------- | -------- | ---------- |
| Wake Word           | `audio.wake`                  | Python   | Planned    |
| Speech Recognition  | `audio.speech`                | Python   | Planned    |
| Conversation Engine | `conv.turn`                   | Python   | Planned    |
| Goal Interpreter    | `goal.raw`                    | Python   | Planned    |
| Intent Engine       | `goal.raw` → `intent.resolved`| Python   | Planned    |
| Planning Engine     | `intent.resolved` → `plan.created` | Python | Planned  |
| Execution Engine    | `plan.created` → `exec.result`| Rust/Py  | Planned    |
| Verification Engine | `exec.result` → `verify.*`    | Python   | Planned    |
| Voice Reply (TTS)   | `voice.speak`                 | Python   | Planned    |
| Vision              | `vision.frame`, `vision.query`| Python   | Planned    |
| Windows Skill       | `win.*`                       | C#       | Planned    |
| Browser             | `browser.*`                   | Python   | Planned    |
| Terminal            | `shell.command`               | Rust     | Planned    |
| Filesystem          | `fs.*`                        | Rust     | Planned    |
| Coding              | `code.*`                      | Python   | Planned    |
| Git                 | `git.*`                       | Rust     | Planned    |
| Discord             | `discord.*`                   | Python   | Planned    |
| Spotify             | `spotify.*`                   | Python   | Planned    |
| Steam               | `steam.*`                     | Rust     | Planned    |
| OBS                 | `obs.*`                       | Rust     | Planned    |
| VSCode              | `vscode.*`                    | TS/Rust  | Planned    |

## Core reasoning engines (frozen design)

The planner is split into four independent engines so each stage is debuggable
and replaceable. They are ordinary bus plugins, not core-specialised code.

| Engine              | Consumes            | Produces                  | Design doc        |
| ------------------- | ------------------- | ------------------------- | ----------------- |
| Intent Engine       | `goal.raw`          | `intent.resolved`         | `docs/Planner.md` |
| Planning Engine     | `intent.resolved`   | `plan.created`            | `docs/Planner.md` |
| Execution Engine    | `plan.created`      | `exec.action`,`exec.result`| `docs/Execution.md` |
| Verification Engine | `exec.result`       | `verify.passed/failed`    | `docs/Planner.md` |

## Reference plugins shipped with the core

`core/examples/echo_plugin.rs` is a working reference implementation that
subscribes to `demo.ping` and emits `demo.pong`. Use it as the template for new
plugins.

## Adding a module

1. Create a crate under `plugins/<name>/`.
2. Implement the `Plugin` trait.
3. Declare `subscribes` / `emits` in `PluginMetadata`.
4. Register it with `Friday::register_plugin`.
5. Document it in this file and in `PluginSDK.md`.
