# R5-2-4 — Context Gate (FROZEN SPEC)

**Status:** FROZEN survey/spec step for R5-2-4. **No implementation in this freeze.** Locks the architecture of the single consent boundary between OS context and any remote provider, and scopes the exact minimal build slice that follows.
**Inputs:** frozen `9c00b2c` (R5-2-3-2, on `dev`), frozen `R5-2-1-PROVIDER-ARCHITECTURE.md` §5/§6.3 (Context Gate position + policy), frozen `R5-2-2` (§43–§108 disclosure duty), frozen `R5-2-3-0` (Copilot/Groq seams).
**Starting point:** `dev` at `9c00b2c`, working tree clean.
**Sequence:** R5-2-1 architecture → R5-2-2 survey → R5-2-3 account-auth → R5-2-3-1 → R5-2-3-2 qualify → **R5-2-4 this spec** → implementation → qualify → freeze → R5-2-5 OS-level reasoning.

---

## 1. What the Context Gate is

The Context Gate is the **single consent boundary between OS context and any provider**. It sits *between* FRIDAY and all **remote** providers (Free tier, Subscription, Paid API). **Local does not cross it** (privacy=Required stays local and needs no remote consent — a feature, not a loophole).

It answers one question before any context channel is allowed to travel to a provider:

> **Did the user authorize FRIDAY to send this kind of information?**

This is distinct from the second, independent question asked at the same outbound boundary:

> **Even if authorized, is this particular payload safe to send?**

That second question is the existing **[EgressGate]** (secret/redaction policy). **The two gates stay independent** — merging them muddies the security model.

```
OS context
   │
   ▼
AgentContext / PromptCompiler
   │
   ▼
┌──────────────────────────────┐
│       ContextGate            │   ← "did the user authorize this KIND?"
│ screen · selection · app     │      per-channel, per-use consent + revoke + disclosure
│ files · mic · clipboard      │
│                              │
│ Allow / Block + reason       │
└──────────────┬───────────────┘
               │   authorized channels only
               ▼
        EgressGate            ← "is this particular payload safe?"
        secret policy
               │
               ▼
       InferenceFabric
       (fabric/provider dispatch)
```

If a context channel is **not authorized, it never reaches EgressGate as provider-bound data.** Blocking at the ContextGate precedes the EgressGate in the outbound order.

---

## 2. The two-gate security model (frozen)

| Gate | Question | Already exists? | R5-2-4 |
|---|---|---|---|
| **ContextGate** | *Did the user authorize FRIDAY to send this kind of information?* | ❌ new | Build (this slice) |
| **EgressGate** | *Even if authorized, is this particular payload safe to send?* | ✅ `egress.rs` | Reuse, untouched |

Both operate at the **same outbound boundary** (the provider dispatch seam already hooked by `InferenceFabric::egress_process`), but they answer different questions and remain composable but separate types.

**Crisp invariants (frozen):**

1. **Provider choice cannot increase the context FRIDAY is allowed to disclose.** Selecting a remote provider grants no channel access that the user has not already consented to per-use.
2. **Remote reasoning cannot obtain a context channel merely because that provider was selected.** Channel consent is orthogonal to provider selection; routing never implies consent.
3. **Local never consents; remote never bypasses.** privacy=Required (Local) payloads do not require remote consent; a remote provider cannot receive an unauthorized channel regardless of routing.

---

## 3. Twelve frozen decisions

1. **Location:** `services/intelligence`, coexisting with `egress.rs` at the provider dispatch boundary.
2. **No new crate.** `services/context-gate` is explicitly rejected — a new crate merely to name an existing boundary is churn.
3. **`ContextChannel`** = `Screen`, `Selection`, `App`, `Files`, `Mic`, `Clipboard` (independent surfaces, per R5-2-1 §5).
4. **Consent is per channel, per use.** A channel is consented for one outbound context request, not blanket-approved for a session/provider.
5. **Revocation takes effect immediately.** A revoked channel cannot be transmitted on any in-flight or subsequent request once the gate re-checks ownership of that channel.
6. **Remote providers require explicit consent.** Any remote (Free/Subscription/Paid) class must have a per-use consent decision for a channel to pass.
7. **Local providers bypass remote-consent requirements.** Local (privacy=Required) payloads do not cross the ContextGate consent check.
8. **Free-tier disclosure is visible before transmission.** When a consent decision is formed, the gate carries disclosure metadata (e.g. free-tier "data improves products" duty from R5-2-2) so the UX can show it before the payload is sent.
9. **ContextGate and EgressGate remain independent.** Separate types, separate questions, no combined transform. `inspect()` does NOT transform payloads.
10. **No provider-specific consent logic.** The gate is agnostic to which remote provider is selected; consent applies identically across the remote class.
11. **No browser/CDP logic enters this phase.** R5-2-WEB remains a separate research branch; this gate is provider-dispatch only.
12. **No OS UI implementation yet.** Only the security contract is built. R5-2-5 builds the OS-level reasoning *experience* on top of this contract. The existing shell seam is used only if the qualification slice requires it.

---

## 4. The contract ContextGate exposes (for the implementation slice)

Frozen shape (not yet implemented — this is the target the implementation slice must meet):

```text
Context request (channel, target-provider class)
      ↓
ContextGate::authorize(channel)      → Decision { Allow | Block{reason} } + disclosure
      ↓  (if Allow)
EgressGate::inspect(provider, payload)  → Allow | Redact | Block   (existing)
      ↓
provider
```

- `inspect()`/`authorize()` returns a **decision + reason + optional disclosure**, and does **not** rewrite the payload (transformation stays with EgressGate if/when it redacts secrets).
- Consent is **explicit, attributable state** (per channel, per use) with revocation — **not** cryptographic signing. Cryptographic signing is out of scope unless a real trust boundary demands it.
- `Remote` (non-local) provider class **must** consult the gate; `Local` class bypasses remote-consent.

*(The exact Rust types — e.g. `ContextChannel` enum, `ContextDecision`, a `ContextGate` that owns a per-session consent set and a `revoke` — are defined in the implementation slice, not this freeze. This doc fixes the security contract, not the API spelling.)*

---

## 5. What R5-2-4 does **not** do (explicit non-goals)

- **No new crate.**
- **No UI/desktop permissions project.** This is the security contract, not a FRIDAY permissions UX.
- **No browser/CDP / web-UI automation.**
- **No provider-specific consent logic.**
- **No cryptographic signing of consent.**
- **No rewriting of EgressGate behavior.**
- **No OS-level reasoning experience** (that is R5-2-5).
- **No blanket OS exfiltration** — the OS is never blanket-exfiltrated; only individually consented channels pass.

---

## 6. Acceptance boundary for R5-2-4 (spec freeze)

- [x] Position per R5-2-1 §5 confirmed as the single boundary between OS context and remote providers.
- [x] Local-bypass invariant recorded (privacy=Required never consents).
- [x] Remote-never-bypasses invariant recorded (routing never implies consent).
- [x] Provider-choice-cannot-increase-disclosure recorded.
- [x] `services/intelligence` chosen; new-crate approach rejected.
- [x] Twelve decisions frozen (§3).
- [x] Two-gate independence (ContextGate vs EgressGate) frozen (§2).
- [x] Committed as this freeze point on `dev`.

**Next:** the R5-2-4 **implementation** slice that builds the contract from §4 (types + per-use consent + revocation + disclosure + local bypass), qualified with tests, then frozen. After that, **R5-2-5 — OS-level reasoning** builds the experience on this contract.

> Note for the implementer: this doc is a **spec freeze only** — `services/intelligence/src` must contain **zero** R5-2-4 code at this commit. The implementation arrives in the next slice.
