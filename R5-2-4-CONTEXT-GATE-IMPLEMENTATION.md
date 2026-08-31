# R5-2-4 — Context Gate (IMPLEMENTATION FREEZE)

**Status:** FROZEN implementation of the R5-2-4 Context Gate security contract.
**Inputs:** frozen spec `R5-2-4-CONTEXT-GATE-SPEC.md`, `dev @ e7e4684`.
**This freeze:** implements the frozen §3/§4 contract in `services/intelligence`; no other subsystem touched (no Omarchy, no OS UI, no browser automation, no provider adapter changes).

---

## What was built

New module `services/intelligence/src/context_gate.rs` (re-exported from `lib.rs`):

| Symbol | Contract |
|---|---|
| `ContextChannel` | `Screen · Selection · App · Files · Mic · Clipboard` — independent surfaces (decision 3), each with a human `label()` for disclosure UI. |
| `ProviderTarget` | `Local · FreeTier · Subscription · PaidApi`; `Local` is the privacy=Required class, everything else is remote (`requires_remote_consent()`). |
| `ContextDecision` | `Allow` \| `Block { reason }` — never transforms the payload (decision 9, §4). |
| `ContextGate` | Owns per-use, per-channel **explicit attributable** consent state (a set). `consent()` / `revoke()` (immediate) / `is_consented()` / `authorize()` / `disclosure_for()`. No cryptographic signing (decision 9). |
| `compose()` | The outbound boundary: **ContextGate → EgressGate** ordering. An unauthorized channel returns `Err` and **never reaches `EgressGate` as provider-bound data**; an authorized channel still independently passes through egress secret policy. |
| `ContextComposeError` | `NotAuthorized { reason, disclosure }` \| `EgressBlocked { reason }`. |

**Key semantics:**

- **Per-use consumption:** `authorize()` on a remote target *consumes* the per-use consent — after one transmission, the same channel is not automatically authorized again (decision 4). `is_consented()` is the non-consuming inspector for disclosure/UI.
- **Immediate revoke (decision 5):** `revoke()` drops pending consent synchronously; a revoked channel fails the next `authorize`.
- **Local bypass (decision 7 / invariant 3):** a `Local` target returns `Allow` with **no** consent consumed or recorded — local never consents. The same channel remains unavailable to any remote provider.
- **Disclosure (decisions 6, 8):** `disclosure_for(channel, target)` is `None` for local, `Some` (free-tier duty, R5-2-2) for every remote target — shown **before** transmission by the caller.
- **Composition (frozen §2):** the two gates remain independent and answer different questions: *did the user authorize this KIND?* (ContextGate) vs *is this particular payload SAFE?* (EgressGate).

---

## Frozen invariants — pinned by tests (12 tests in `context_gate.rs`)

1. **Provider choice cannot increase the context FRIDAY is allowed to disclose.**
   `remote_provider_selection_gives_no_unconsented_channel`, `channel_consent_is_per_use_not_per_provider`
2. **Remote reasoning cannot obtain a channel merely because that provider was selected (routing never implies consent).**
   `routing_never_implies_consent`
3. **Local never consents; remote never bypasses.**
   `local_bypasses_remote_consent_without_consuming_or_recording`, `local_and_remote_are_orthogonal`

Plus: `revocation_is_immediate`, `consent_is_per_channel`, `remote_carries_disclosure_local_does_not`, and three composition tests
(`compose_blocks_unauthorized_before_egress`, `compose_authorizes_then_egress_independently_blocks_secret`, `compose_allows_authorized_and_safe_payload`, `compose_local_bypasses_consent_and_still_runs_egress`).

---

## Verification

- `cargo test -p friday-intelligence` → **93 lib tests** (81 prior + 12 new) + **11 integration** tests pass.
- `cargo clippy -p friday-intelligence --all-targets -- -D warnings` → clean.
- `cargo fmt -p friday-intelligence` → clean.
- Full workspace `cargo build` → clean; no `mobile/` or other cross-crate pollution (diff is scoped to `lib.rs` + new `context_gate.rs`).
- Frozen spec's own non-goals honored: no new crate, no UI, no CDP, no provider-specific consent, no signed consent, no egress rewrite.

---

## Scope guard

This freeze touches **only**:
- `services/intelligence/src/context_gate.rs` (new)
- `services/intelligence/src/lib.rs` (module + re-exports)

Next stone: **R5-2-5 — OS-level reasoning**, the experience layer built on this contract, where the Context Gate becomes visible and useful ("*Friday, see what I'm looking at and help me with it*") — without building a forbidden ChatGPT-web proxy.
