# Qualification

Qualification measures a capability's **recovery maturity** — its ability to keep
the user successful when its provider fails.

## Lifecycle & objective exit criteria

Every recovery policy climbs:

```
Prototype → Shadow → Limited → Production → Qualified
```

- **Prototype** — recovery policy defined for the capability's failure categories;
  unit tests pass; offline simulation resolves at least one category.
  `RecoveryMode = Off`.
- **Shadow** — graph runs without affecting execution and agrees with actual
  outcomes on ≥ 90% of injected failures over a fixed task window, with zero safety
  incidents. `RecoveryMode = Shadow`.
- **Limited** — Active recovery for internal/opt-in users; observed Recovery
  Effectiveness within tolerance of predicted confidence; coverage trending up.
  `RecoveryMode = Active` (scoped).
- **Production** — Active recovery for all users; meets the qualification bar
  (coverage ≥ 85%). `RecoveryMode = Active`.
- **Qualified** — recovery coverage ≥ 85% **sustained** over a defined window
  (e.g. trailing 7 runs); confidence model stable (predicted ≈ observed). The
  Resilience Dashboard shows `Qualified: YES`.

The runtime `RecoveryMode` (`Off` / `Shadow` / `Active`, in
`services/recovery/src/taxonomy.rs`) selects how far along that ladder the system
operates. Production users opt into `Active` only after shadow agreement.

## Qualified flag

A subsystem is **Qualified** when its recovery coverage (resolved / attempts) is
≥ 85% sustained. The Resilience Dashboard marks it `Qualified: YES`.

## Distinction from Certification

Qualification is narrowly about *recovery*. **Certification** (see
`certification.md`) is the broader engineering sign-off for a whole capability and
is a separate gate — and requires Qualification as a prerequisite.

## Client Qualification

The same evidence-first discipline now applies to **external clients** of the
Mobile Protocol (`services/mobile`). A client is qualified only after it builds,
installs, lints, and demonstrates end-to-end against the live daemon — not when
its code is merely authored. Gates and the canonical **Demo 001** are in
`specs/client-qualification.md`; the protocol itself is a versioned API documented
in `clients/README.md`.
