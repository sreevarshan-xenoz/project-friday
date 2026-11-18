# FRIDAY Console (Android) — M6.2b reference client

The **first official FRIDAY client**. A thin, protocol-driven console for the
FRIDAY Mobile Protocol (`services/mobile`). It knows only protocol DTOs — never
FRIDAY internals (no TaskGraph, RecoveryGraph, provider routing, or AgentKernel).

> **Build status:** ✅ **build-verified** (2026-07-15). `assembleDebug` →
> BUILD SUCCESSFUL, `app-debug.apk` produced, `lintDebug` → 0 errors (18 benign
> warnings). Built with Gradle 8.2 + JDK 17 + SDK `platform-34`/`build-tools
> 34.0.0`. The only remaining gate is the on-device **Demo 001** (real J7). The
> Rust `friday-mobile` side it talks to **is** built, clippy-clean, and
> test-verified (19 tests).

## Stack
- Kotlin + Jetpack Compose (Material 3)
- Navigation Compose
- OkHttp WebSocket
- kotlinx.serialization (JSON)
- DataStore (pairing token)
- Coil (reserved for future screenshots)

## Architecture (exactly the requested split)
```
protocol/   wire DTOs — mirror services/mobile/src/protocol.rs 1:1
network/    FridayWebSocket (OkHttp) + connection status types
data/       SessionRepository (DataStore token) + AppContainer (transport plumbing)
domain/     pure models + protocol → UI reduction (no Android, no FRIDAY)
ui/         theme · components · viewmodel · screens · MainActivity · LockScreen
notifications/  M6.2.6 notification actions → protocol commands
```

## Wire protocol (must match `services/mobile`)
| Frame | Direction | Notes |
|-------|-----------|-------|
| `welcome` | S→P | `requires_auth` — an unpaired phone then says `hello` |
| `auth_challenge` | S→P | `challenge` only. The pairing code is shown on the PC, never sent; the user types it on the phone |
| `auth_response` | P→S | `code` (typed) + `signature = sha256(challenge \|\| code)` |
| `auth_complete` | S→P | `token` — persisted in DataStore |
| `event` | S→P | `topic` + `payload`; bridge tags `_kind` / `_source` |
| `telemetry` | S→P | real host CPU/RAM/Disk (`SystemProbe`) |
| `command` | P→S | `method` (`planner.goal`, `task.*`, `system.info`) + `params` |
| `command_result` | S→P | ack for a `command` |

Tag style is `serde(tag="type", rename_all="snake_case")`, so the Kotlin sealed
interfaces use `@JsonClassDiscriminator("type")` + `@SerialName("snake_case")`.

## Server-side changes made alongside this client
- `transport.rs`: the server sends `auth_challenge` (a per-connection challenge, no
  code) to an unauthenticated `Hello`. The pairing code is shown on the PC.
  The link is not encrypted yet: pair on a network you trust, not shared Wi-Fi.
- `bridge.rs`: forwarded events now carry `_kind` and `_source` so a thin client
  can derive task status (`task.started` vs `task.completed`) without internals.
- `protocol.rs`: the handshake now carries `protocol: "1.0"` (`PROTOCOL_VERSION`)
  in both `Hello` and `Welcome`, so incompatible clients are rejected/negotiated
  instead of guessed at.
- These are covered by the existing `friday-mobile` test suite (17 tests, clippy
  `-D warnings` clean).

## Protocol versioning
The Mobile Protocol is an **API**. The handshake exchanges `protocol` (currently
`"1.0"`) in both directions; bump it deliberately (`1.0 → 1.1 → 2.0`). New fields
use defaults so older clients keep working. See `../../clients/README.md`.

## Connection states (explicit, never guessed)
`Disconnected · Connecting · Authenticating · Connected · Degraded ·
Reconnecting`. The launcher renders exactly these; failures move to
`Reconnecting` (bounded auto-retry) rather than a silent dead state.

## Milestones covered
- **M6.2b.1** Foundation — full Gradle/Compose project, package layout.
- **M6.2b.2** Pairing — connect → `auth_challenge` → tap Pair → `auth_complete`
  → token saved → auto-reconnect on next launch.
- **M6.2b.3** Dashboard — PC/Planner status, live CPU/RAM/Disk, current goal,
  running count.
- **M6.2b.4** Task Viewer — task list from `event` stream; tap → structured
  Execution/Planner/Recovery/Logs/Evidence sections.
- **M6.2b.5** Command Console — one text box → `planner.goal` → bus → planner →
  execution → `command_result`. No AI on the phone.
- **Launcher** — `MainActivity` is registered as `HOME` + `LAUNCHER`.
- **Lock screen** — `LockScreenActivity` shows last task over the keyguard.
- **M6.2.6** Notifications — failed tasks raise a notification with
  [Open]/[Retry]/[Ignore] that emit real protocol commands.

## Not yet built (per the "don't add yet" list)
Chat history, AI avatars, animations, LLM settings, model picker, prompt
engineering. Files/Terminal-browse and live task pause/cancel/approve buttons are
scaffolded but wired to M6.2c/M6.2e slices.

## How to build / run
**From the command line (no Android Studio needed):**
```powershell
$env:JAVA_HOME = "C:\Program Files\Eclipse Adoptium\jdk-17.0.17.10-hotspot"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
cd clients/android
.\gradlew assembleDebug      # -> app/build/outputs/apk/debug/app-debug.apk
.\gradlew lintDebug          # -> 0 errors
```
The Gradle wrapper (`gradle/wrapper/gradle-wrapper.jar`) is committed, so
`./gradlew` works once `JAVA_HOME` + `ANDROID_HOME` are set. Compose is enabled
via `composeOptions { kotlinCompilerExtensionVersion = "1.5.10" }` (the
`org.jetbrains.kotlin.plugin.compose` plugin only exists from Kotlin 2.0; for
`1.9.22` this is the correct wiring).

**On a device:**
1. Start the PC daemon: `cargo run -p friday-mobile --bin friday-mobile-daemon`
   (listens on `0.0.0.0:18432` by default; set `FRIDAY_MOBILE_ADDR` to change).
2. Install + run on the J7 (or any API 24+ device):
   `adb install -r app/build/outputs/apk/debug/app-debug.apk`. On first launch
   enter the PC's LAN WebSocket URL, e.g. `ws://192.0.2.10:18432`.
3. Pair, watch live CPU/RAM, send `Open Notepad`, and watch the task complete.

## Demo 001 (canonical Alpha demo)
Power on the J7 → launcher auto-starts → shows `PC 🟢 Connected` → live CPU/RAM
updates → type `Open Notepad` → Planner receives → Windows executes → task
completes → phone shows `✓ Open Notepad` + `Latency 2.1 s`.

That single flow validates the protocol, auth, transport, telemetry, command
routing, planner, Windows bridge, execution, and the UI at once. Full qualification
gates (Build · APK · Lint · Instrumentation · Protocol · Manual Demo) are in
`../../specs/client-qualification.md`.

## Client Qualification
This client is **not** merged until Demo 001 passes on real hardware and the five
merge verifications are recorded (see `../../specs/client-qualification.md`). Until
then the Android client is a design artifact, not evidence. The UI is intentionally
frozen after Demo 001 — no new screens until the desktop platform matures.
