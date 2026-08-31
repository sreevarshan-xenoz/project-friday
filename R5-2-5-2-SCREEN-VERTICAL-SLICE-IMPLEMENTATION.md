# R5-2-5-2 — Screen Vertical Slice (IMPLEMENTATION FREEZE)

**Status:** FROZEN implementation of the R5-2-5-2 daemon integration — the first end-to-end screen-context read.
**Inputs:** frozen R5-2-5-2-0 survey (`a415e2e`), frozen collector (`e3cdba7`), frozen ContextGate/EgressGate (`19ec6b8`), `dev @ a415e2e`.
**This freeze:** wires `ContextCollector → ContextGate → EgressGate → provider` for one explicit user-requested Screen capture through the daemon's command-ingress surface. No shell chooser, no ambient capture, no clipboard/files/mic/OCR, no new IPC, no ChatGPT web-session.

---

## What was built

### New module: `services/mobile/src/context_read.rs`

The orchestration seam that composes the three frozen layers into one honest vertical slice:

| Symbol | Contract |
|---|---|
| `ProviderSeam` (trait) | `async fn send(&str) -> Result<String, String>` — the **actual provider boundary**. Tests inject a `RecordingSeam` that counts invocations and records payloads, so both invariants are proven **at the seam** (not a boolean upstream). |
| `FabricProviderSeam` | Real daemon implementation. Wraps `Arc<InferenceFabric>`, calls `complete_tier(provider, VisionFree, payload, ...)`. Qualified on the Omarchy target (R5-2-5-4). |
| `ContextReadError` | `Capture(String)` \| `InvalidPayload(String)` \| `NotAuthorized{reason, disclosure}` \| `EgressBlocked{reason}` \| `Provider(String)` — no "happy" variant; errors are the product of the gate chain. |
| `ScreenReader<C,P>` | `new(collector, provider, provider_name)` / `consent()` / `revoke()` / `is_consented()` / `read()`. Builds a `SCREEN_CAPTURE` payload (base64-encoded PNG) from the collector, then calls `friday_intelligence::compose()` — ContextGate → EgressGate ordering enforced. Only calls `provider.send()` if **both** gates allow. |
| `screen_payload()` | Turns `CapturedContext::Image` into a `SCREEN_CAPTURE mime=... geometry=... data=<base64>` text payload for the string-based gate chain. Full image/vision delivery is qualified in R5-2-5-4. |

### Daemon wiring: `friday-linux-daemon.rs` (integration surface only)

- Constructs `ScreenReader<RealCommandRunner, FabricProviderSeam>` before the GoalRunner is started.
- Intercepts `context.read` in the ingress loop **after** the token gate and **before** generic `CommandGateway::dispatch`.
- Consent is **request-attributable**: `params.consent: true` grants per-use consent for that read only. Without it, `read()` is blocked at the ContextGate and the provider sees nothing.
- Records the outcome to `state.json` via a new `ContextReadRecord` in the shell-state read-model.
- Provider name is configured via `FRIDAY_CONTEXT_PROVIDER` env (defaults to `"fabric"`).

### Shell state: `shell_state.rs`

- New `context_read: Option<ContextReadRecord>` field in `ShellState`.
- New `ContextReadRecord { method, ms, ok, channel }` struct surfaced to the shell read-model.
- Pre-existing `doc list item overindented` lint in the `apply_event` doc block fixed (same file, required to unblock the lib clippy gate; no other change to the surrounding code).

### Pre-existing clippy lint fixes (same-file, same bin, required for gate)

- Five `&*st` → `&st` deref fixes in `friday-linux-daemon.rs` write_state calls (`clippy::explicit-auto-deref`). These are mechanical, zero-behavior changes in the exact file this slice modifies; they unblock `cargo clippy -p friday-mobile --bin friday-linux-daemon -- -D warnings`.

---

## Frozen invariants — pinned by tests (7 tests in `context_read.rs`)

**Invariant 1: consented screen reaches the provider.**
`consented_screen_reach_provider` — asserts `provider.sent() == 1` and the payload contains the base64-encoded screen bytes at the `RecordingSeam` (the actual provider boundary).

**Invariant 2: NOT consented → provider receives NOTHING.**
`unconsented_screen_never_reaches_provider` — asserts `provider.sent() == 0` and `provider.last().is_none()` when no consent is granted, proving the ContextGate blocked before the provider seam.

**Consent is per-use, not per-session:**
`consent_is_per_use_consumed_by_read` — one `read()` consumes the grant; a second `read()` is blocked (`provider.sent() == 1`).

**Revocation is immediate:**
`revoke_is_immediate` — consent then revoke ⇒ `provider.sent() == 0`.

**Capture failure surfaces honestly:**
`capture_failure_is_reported_not_fabricated` — `FakeCommandRunner` fails loudly → `ContextReadError::Capture`.

**Provider error surfaces after gates allow:**
`provider_error_surfaces_after_gates_allow` — `RecordingSeam::failing()` returns `Err` after the gates allowed the read → `ContextReadError::Provider` and `provider.sent() == 1`.

**Payload correctness:**
`fullscreen_read_builds_correct_payload` — `SCREEN_CAPTURE` format, geometry, and mime are correct.

---

## Verification

| Gate | Result |
|---|---|
| `cargo test -p friday-mobile` | **79 lib** + **1 integration** pass (includes 7 new `context_read` tests). |
| `cargo test -p friday-context-collector` | **15 tests** pass (unchanged from R5-2-5-1). |
| `cargo clippy -p friday-mobile --lib --bin friday-linux-daemon -- -D warnings` | Clean (scope: this slice's surface). |
| `cargo build` (workspace) | Clean. |
| `friday-ctl.rs` | **Not touched** (belongs to R5-2-5-3 shell chooser). Has 1 pre-existing `clippy::collapsible_match` lint (same as R5-2-5-2-0 baseline). |

**fmt note:** `shell_state.rs` and `friday-linux-daemon.rs` retain the committed non-rustfmt formatting (consistent with R5-2-5-2-0's baseline disposition). Running `cargo fmt` on these files would reflow unrelated code — an `add7219`-style contamination the survey explicitly guards against.

---

## Scope guard

This freeze touches **only**:
- `services/mobile/src/context_read.rs` (new)
- `services/mobile/src/bin/friday-linux-daemon.rs` (imports + ScreenReader construction + context.read handler + 5 deref fixes)
- `services/mobile/src/commands.rs` (1 const + doc)
- `services/mobile/src/lib.rs` (1 module declaration)
- `services/mobile/src/shell_state.rs` (field + struct + starting init + doc-lint fix)
- `services/mobile/Cargo.toml` (1 dep)
- `Cargo.lock` (lockfile)

It does **not** touch: `friday-ctl.rs`, `shell_contract.rs`, `context-collector`, `intelligence`, or any other subsystem. The `ProviderSeam` trait is the architectural seam; the daemon's thin wiring is the only consumer in this slice.

**Not in scope for R5-2-5-2 (deferred):**
- Shell context chooser (R5-2-5-3)
- Ambient / continuous screen capture (forbidden)
- Linux/Omarchy real capture qualification (R5-2-5-4) — on Windows CI the `FakeCommandRunner` + `RecordingSeam` prove the orchestration; real `grim`/Wayland capture is qualified on the Omarchy target
- Clipboard / Files / Mic / OCR / Selection channels
- New IPC / new provider architecture / ChatGPT web-session

Next stones: **R5-2-5-3** (shell context chooser) → **R5-2-5-4** (Linux/Omarchy real capture qualification) → R5-2-5 frozen.
