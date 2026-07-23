# Alpha Integration Week

**Declared:** 2026-07-15 (after the Android client reached `BUILD SUCCESSFUL` +
`lintDebug` clean — see `specs/client-qualification.md`).
**Status:** 🟡 Scheduled — *not a coding milestone*. No M6.3 / Voice / Vision /
Browser work during this week.
**Owner:** Reliability Engineering (you, dogfooding).
**Rule:** Prove the *system* integrates before adding any new capability. FRIDAY
stops being a collection of subsystems and starts behaving like a product only
after it survives repeated, deliberate, end-to-end stress and recovery.

This week exists because Demo 001 passing *once* proves the plumbing works.
It does **not** prove the product is trustworthy. Integration Week is the trust
gate between "developer-facing" and "user-facing".

## The stance

- **Stop writing features.** No M6.3, no Voice, no Vision, no Browser.
- **Run the system, don't build it.** Every day is a demo against the live
  daemon + the J7.
- **Record everything.** Phone screen, desktop screen, timestamps, WebSocket
  logs, planner logs. Demos 002/003/010 accumulate into a maturity record.
- **One threshold to clear:** after the week, you should be able to control your
  PC from the phone *without hesitation*. If you still hesitate, that hesitation
  is the bug to fix next — not a new feature.

## Day 1 — Demo 001 (canonical Alpha)

Exactly as planned (see `specs/client-qualification.md`):

```
J7 boots
   ↓  FRIDAY Launcher
   ↓  QR Pair
   ↓  Live Dashboard
   ↓  Open Notepad
   ↓  Task Completed
```

- If it works **once**: good.
- If it works **ten times**: better.
- Record: phone screen, desktop screen, timestamps, WS logs, planner logs.

## Day 2 — Stress Demo

Do **not** open Notepad once. Run a tight loop and watch for integration rot:

```
Open Notepad · Close · Open · Focus · Type · Clipboard · Repeat ×100
```

Watch for:
- memory leaks (daemon RSS, phone retained objects)
- WebSocket disconnects / reconnection storms
- stale sessions (zombie `MobileSession` entries in `SessionRegistry`)
- duplicate events (same task id delivered twice)
- race conditions (command ack vs event ordering)

## Day 3 — Recovery Demo

Deliberately break things *while the phone is connected*:

- Kill the Windows daemon.
- Kill the mobile daemon.
- Disconnect Wi-Fi.
- Restart FRIDAY.
- Restart the PC.
- Kill OmniRoute.

Observe the target path with **no user intervention**:

```
Launcher → Reconnect → Recover → Continue
```

The explicit connection states (`Disconnected · Connecting · Authenticating ·
Connected · Degraded · Reconnecting`) exist precisely so this is observable and
bounded (max 3 auto-retries). This is the day those states earn their keep.

## Day 4 — Dogfood

Control FRIDAY **only from the phone** for one hour.

- No terminal. No desktop shortcuts.
- If you instinctively reach for the keyboard, **write down why**.
- That list is the **Friction Log** — the highest-signal input for what to build
  after Beta.

### Micro-task surfaced during dogfood (deferred, tiny)
Add a **status ribbon** to the launcher (proposed, not yet built):

```
══════════════════════════════
FRIDAY
PC Connected
Protocol v1.0
Latency 42 ms
══════════════════════════════
```

Tiny feature, huge value: the first thing you'll check when something feels
"off". Logged here so it isn't lost; implement only if dogfood confirms the need.

## Day 5 — Trust Week begins

You already defined Trust Week. Now actually start it:

- Every development task goes through FRIDAY first.
- If FRIDAY can't do it, **record it** — don't bypass silently.
- The Friction Log + Trust Week entries feed the post-Beta roadmap.

## Before Beta — Integration Qualification

Windows / Provider / Recovery Qualification already exist. Add a fourth, because
**this is where distributed systems usually fail** — the seams between subsystems,
not the subsystems themselves. See `specs/integration-qualification.md`.

| Metric                     | Target |
|----------------------------|--------|
| Mobile ↔ Daemon Connection | >99%   |
| Command Round-trip Success | >99%   |
| Event Delivery             | 100%   |
| Session Recovery           | >95%   |
| Notification Reliability   | >99%   |

## Reproducible artifacts

Every demo must be reproducible. Generate a versioned, checksummable release
under `artifacts/android/0.1.0-alpha/` (see `artifacts/android/0.1.0-alpha/
README.md`):

```
artifacts/android/0.1.0-alpha/
   app-debug.apk
   sha256.txt
   build-info.json
```

Now every demo is tied to an exact binary.

## Exit criteria (Alpha Integration Week → Beta candidate)

- [ ] Demo 001 recorded (phone + desktop + logs), ten-repeat variant stable.
- [ ] Stress Demo: no leaks / disconnect storms / duplicate events observed.
- [ ] Recovery Demo: every kill path returns to `Connected` unaided.
- [ ] Friction Log written from a full phone-only hour.
- [ ] Integration Qualification baseline captured (see its spec).
- [ ] Reproducible artifact published with sha256 + build-info.
- [ ] Subjectively: you trust the phone enough to use it without hesitation.
