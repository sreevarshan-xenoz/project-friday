# Integration Qualification — pre-Beta gate

**Status:** 🟡 Defined (Alpha Integration Week). Not yet measured.
**Owner:** Reliability Engineering
**Companion:** `specs/alpha-integration-week.md`, `specs/client-qualification.md`
**Why this spec exists:** Windows, Provider, and Recovery Qualification already
gate individual subsystems. They do **not** gate the *seams* between them.
Distributed systems almost always fail at the boundaries — the phone-to-daemon
link, the command round-trip, event delivery, session recovery, notifications —
not inside a single well-tested crate. Integration Qualification measures the
product as a whole.

This is the fourth qualification gate, sibling to the other three. A client is
not "done" until `specs/client-qualification.md` passes; the *platform* is not
Beta-ready until this spec's targets hold under sustained real use.

## Metrics

| Metric                     | Target | What it measures                                      |
|----------------------------|--------|-------------------------------------------------------|
| Mobile ↔ Daemon Connection | >99%   | Fraction of wall-clock time a paired phone is `Connected` (excludes intended offline). |
| Command Round-trip Success | >99%   | `command` → `command_result` (or observable effect) without loss/timeout. |
| Event Delivery             | 100%   | Every `ServerMessage::Event` the server emits is received and reduced exactly once. |
| Session Recovery           | >95%   | Of all disconnects/kills, fraction that auto-return to `Connected` with no user intervention. |
| Notification Reliability   | >99%   | M6.2.6 notifications (Open/Retry/Ignore) delivered + their emitted commands observed server-side. |

## How each metric is observed (no guessing)

- **Connection %** — sample `ConnectionStatus` on the phone over the dogfood hour;
  the launcher's status ribbon (`alpha-integration-week.md`, Day 4 micro-task)
  makes this a glanceable signal. Cross-check against `SessionRegistry` on the
  daemon.
- **Round-trip** — the command console already correlates `command.id` →
  `command_result.id`. Log both; count mismatches.
- **Event Delivery** — `BusBridge` is the single forwarder. Count events emitted
  by the bridge vs events received/reduced on the client; dedup by event id to
  catch duplicates (a Day-2 Stress Demo concern).
- **Session Recovery** — instrument `Reconnecting → Connected` transitions; the
  bounded retry (max 3) lives in `MainViewModel` / `FridayWebSocket`.
- **Notification Reliability** — emit a known `Retry` from a failed-task
  notification; assert the corresponding `command` appears on the daemon bus.

## Acceptance (Beta candidate)

- [ ] A measurement harness exists for all five metrics (even if manual/log-based
      for Alpha — automation is post-Beta).
- [ ] Sustained run (Alpha Integration Week, Day 2–3) meets every target, or
      deviations are documented as known issues with owners.
- [ ] Results recorded alongside Demo 001 artifacts under `artifacts/android/`.

## Relationship to other specs

- Does **not** modify `protocol.rs` (API frozen this phase).
- Consumes the outputs of Windows / Provider / Recovery Qualification.
- Consumes `specs/client-qualification.md` (the client must first build + demo).
- Feeds the post-Beta roadmap via the Alpha Integration Week Friction Log.
