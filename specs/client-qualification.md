# Client Qualification — M6.2b

**Status:** 🟡 Under Qualification (Android reference client **builds + lints green**, APK produced; on-device Demo 001 pending)
**Owner:** Reliability Engineering
**Client under test:** `clients/android` (FRIDAY Console, the first external FRIDAY client)
**Rule:** Evidence-based. The mobile protocol is now an **API**. A client is not
"done" until it is built, installed, and demonstrated end-to-end against the live
daemon. Uncompiled code is a design artifact, not evidence.

This spec is the qualification plan for FRIDAY **clients**. It does not change
core contracts. It adds a **build/lint/demo pipeline** so the Android client —
and every future client — must pass the same bar Windows, Filesystem, Git, and
providers already do.

## Why clients, and why now

M6.2a proved the daemon serves real telemetry and round-trips structured
commands. M6.2b produced the first external client. FRIDAY is no longer "an
assistant" — it is a **distributed platform** with a Mobile Protocol surface. That
surface must be qualified like any other subsystem: built, linted, installed, and
demoed, not merely authored.

## Scope — what "qualified" means for a client

- **Android Build** — the project compiles (`assembleDebug` / `assembleRelease`).
- **APK** — a runnable artifact is produced and installs on the target device (J7, API 24).
- **Lint** — `./gradlew lint` is clean (no errors; warnings triaged).
- **Instrumentation** — at least a smoke test launches the app and reaches the
  pairing screen; protocol round-trip covered by a test or manual check.
- **Protocol** — the client speaks exactly `services/mobile/src/protocol.rs`
  (tag `type`, `snake_case`, `protocol: "1.0"` handshake). DTOs mirror it 1:1.
- **Manual Demo** — Demo 001 below runs against the live daemon, end-to-end.

## Build verification (this workspace, 2026-07-15)

The client is **no longer unbuilt**. It was compiled and linted here:

- **Toolchain:** Gradle 8.2 (wrapper generated, `clients/android/gradle/wrapper`),
  Eclipse Adoptium JDK 17, Android SDK `platform-34` + `build-tools 34.0.0`, `adb`
  present. (No Android Studio needed for CLI build.)
- **`assembleDebug` → BUILD SUCCESSFUL** in ~30s (deps cached). Produces
  `app/build/outputs/apk/debug/app-debug.apk`.
- **`lintDebug` → BUILD SUCCESSFUL**, **0 errors** (18 benign warnings: newer
  dependency versions available, one unused color resource, missing monochrome
  launcher icon — none block).
- **Compose wiring note:** for Kotlin `1.9.22` + AGP `8.2.2` the Compose compiler
  is enabled via `composeOptions { kotlinCompilerExtensionVersion = "1.5.10" }`
  in `app/build.gradle.kts`. The `org.jetbrains.kotlin.plugin.compose` Gradle
  plugin only exists from Kotlin 2.0.0, so it is **not** used here.
- Fixed during verification: 5-arg `ParsedEvent` ctor call, missing
  `androidx.navigation.compose.NavHost` import, missing
  `kotlinx.coroutines.flow.update` import, explicit `ClientMessage.serializer()`
  for `encodeToString`, and `Build.VERSION.SDK_INT` guard around
  `NotificationChannel` (minSdk 24 / API 26 API).

### Gate status

| # | Gate          | Status (workspace)                                            |
|---|---------------|---------------------------------------------------------------|
| 1 | Android Build | ✅ `assembleDebug` BUILD SUCCESSFUL                           |
| 2 | APK           | ✅ artifact produced (install/launch on J7 still to do)       |
| 3 | Lint          | ✅ `lintDebug` 0 errors                                       |
| 4 | Instrumentation| ⚪ not written (no `androidTest` suite yet)                  |
| 5 | Protocol      | ✅ DTOs mirror `protocol.rs` and compile; wire-tag/version confirmed |
| 6 | Manual Demo   | ⏳ Demo 001 pending real-hardware run                         |

## Demo 001 (canonical Alpha demo)

Exactly one demo defines the Alpha. Do not demo more until this is reliable.

```
Power on the J7
   ↓  launcher starts automatically
Shows:  PC 🟢 Connected
   ↓  live CPU/RAM updates
Type:  Open Notepad
   ↓  Planner receives
   ↓  Windows executes
   ↓  Task completes
Phone shows:  ✓ Open Notepad    Latency 2.1 s
```

That single flow validates: the mobile protocol, authentication, WebSocket
transport, telemetry, command routing, the planner, the Windows bridge,
execution, and the client UI — all at once. If Demo 001 succeeds, the **Mobile
Platform Alpha** is born. If it fails, that is exactly the operational evidence
this process is designed to collect.

## Merge gate (the five verifications)

Before this client is merged, all five must pass on real hardware:

1. **APK builds.**
2. **QR pairing works** (challenge → signature → token → reconnect).
3. **Live telemetry updates** (CPU/RAM move in real time).
4. **Command `Open Notepad` works** (goal → planner → Windows → complete).
5. **Notification action `Retry` actually executes** (emits a real protocol command).

When all five pass, Android is **🟢 Qualified**.

## Acceptance criteria (M6.2b exit → 🟢)

- [x] Android project builds clean (`assembleDebug` BUILD SUCCESSFUL; `assembleRelease`
       not yet run — release signing not configured).
- [x] `./gradlew lint` clean (errors zero; 18 warnings reviewed/benign).
- [ ] At least one instrumentation/smoke test exercises launch + handshake.
- [x] Client DTOs verified against `services/mobile/src/protocol.rs` (protocol `1.0`),
       and they compile (sealed-interface polymorphic serialization confirmed).
- [ ] Demo 001 performed on the J7 against `friday-mobile-daemon`; five merge
       verifications recorded.
- [ ] Client UI frozen after Demo 001 (no new screens until the desktop platform
       matures — per the "intentionally boring phone" directive).

## Relationship to other specs

- Does **not** modify `protocol.rs` (the API is frozen this phase; versioned, not
  casually changed).
- Follows `provider-qualification.md` / `windows-qualification.md` (same
  evidence-pack discipline, same graduation gate).
- Progress is tracked per `engineering/qualification.md` (trust, not intelligence).
- Future clients (`clients/desktop`, `clients/web`, `clients/tablet`) reuse the
  same six gates; the protocol is the shared contract.
- Pre-voice item **M6.2.7 Session Transfer** (Desktop ⇄ Phone task handoff) is
  logged as the next client capability; not part of Alpha.
