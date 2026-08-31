# R5-2-5-2-0 — Daemon Integration Ownership & State Survey (FREEZE)

**Status:** FROZEN survey of the R5-2-5-2 integration surface.
**Baseline:** `dev @ e3cdba7` (R5-2-5-1 collector frozen), **working tree clean**.
**Purpose:** establish the integration boundary and record the dirty-tree disposition **before** writing a single line of daemon code — so we never repeat the `add7219` contamination.

---

## The integration boundary (confirmed, architectural)

```text
User explicitly asks FRIDAY for context
              │
              ▼
       Command / request
              │
              ▼
     ┌─────────────────┐
     │ ContextCollector │  ← OS capability only
     └────────┬────────┘
              │ CapturedContext
              ▼
        ContextGate       ← user authorization
              │
              ▼
        EgressGate        ← payload safety
              │
              ▼
     Reasoning Provider
```

Three ownership rules that this survey pins for the whole R5-2-5 series:

1. **The collector does not know** whether context is allowed to leave the machine.
   It is a pure capability (`services/context-collector`, frozen R5-2-5-1): given a channel it returns `CapturedContext { Image | Text | AppInfo }` or a `CaptureError`. Its output vocabulary has **no** allow/consent/provider/privacy variant (structural test `captured_context_has_no_privacy_verdict`).

2. **The daemon does not know how to capture OS context.** The daemon (`services/mobile/src/bin/friday-linux-daemon.rs`) remains the **orchestrator only**. It wires the chain
   `→ collector → ContextGate → EgressGate → provider` and owns the user-request routing and consent presentation — never the capture mechanics.

3. **The shell does not become an orchestration layer.** `friday-ctl.rs` / the shell chooser only *request* context; the daemon composes the chain. (The chooser itself is deferred to R5-2-5-3.)

**Narrow first vertical slice (deliberately constrained history of R5-2-5-2):**

- One **explicit** context request triggered by a user command — **no ambient/continuous context collection** (the architectural line that keeps FRIDAY from becoming a desktop surveillance daemon).
- **Screen** is the first end-to-end channel — the most important proof of the eventual "*Friday, look at what I'm seeing*" experience.
- No shell chooser yet (R5-2-5-3). No microphone/files (collector returns `Unavailable`; not wired end-to-end). No ChatGPT-web proxy. **No daemon rewrite** — the existing command-ingress surface (`CommandGateway`, `WsServer`, `state.json` writer) is reused.

---

## Known interfaces (from frozen crates — survey facts, not new code)

| Concern | Where it lives today | Signature / note |
|---|---|---|
| OS context capture | `friday-context-collector` (R5-2-5-1) | `ContextCollector<R: CommandRunner>::capture(channel, &CaptureOptions) -> Result<CapturedContext, CaptureError>`. `RealCommandRunner` (OS) / `FakeCommandRunner` (Windows CI). |
| Consent | `friday_intelligence::ContextGate` (R5-2-4) | `consent(channel)`, `revoke(channel)` (immediate), `authorize(channel, target) -> ContextDecision`. Per-use per-channel explicit consent. |
| Egress safety | `friday_intelligence::EgressGate` (R5-2-4) | `process(provider, payload) -> Result<String, String>` — independent of ContextGate. |
| Gate composition | `friday_intelligence::compose` (R5-2-4) | `compose(gate, egress, channel, provider, target, payload) -> Result<String, ContextComposeError>`. Enforces **ContextGate before EgressGate** ordering. |
| Provider class | `friday_intelligence::{ProviderTarget, ModelProvider}` | `Local` = privacy=Required (never consents); `FreeTier/Subscription/PaidApi` = remote (require explicit consent + free-tier disclosure). |
| Command ingress | `services/mobile` | `method::PLANNER_GOAL = "planner.goal"`, `SYSTEM_INFO = "system.info"` on `CommandGateway`/`WsServer` (loopback 127.0.0.1:18432, token-gated). |
| Shell state | `services/mobile/src/shell_state.rs` | `state.json`-backed `ShellState` + `apply_event` — reused unchanged for surfacing a context capture result to the shell. |

---

## Dirty-tree disposition (the reason this survey exists)

Before freezing, four files under `services/mobile/` were dirty:

```
services/mobile/src/bin/friday-ctl.rs
services/mobile/src/bin/friday-linux-daemon.rs
services/mobile/src/shell_state.rs
services/mobile/tests/shell_contract.rs
```

**Triaged as category 2 — accidental formatting drift (not content):**

- Every hunk was pure `cargo fmt` churn (import re-sorting, line-wrapping, array reflow) with **zero** logic change.
- Proven lossless: the dirty blobs were byte-identical to plain `cargo fmt --all` output on the frozen `e3cdba7` tree (identical `git index` blob hashes per file).
- Root cause: the R5-2-5-1 qualification ran a workspace-wide `cargo fmt`, which reflowed these older files to the current rustfmt import-sort/width rules.
- **Not** unfinished R5-1/R5-2 work. **Not** the beginning of R5-2-5-2.

**Disposition:** reverted all four to the frozen `e3cdba7` baseline (lossless — no code discarded). Baseline verified clean: `git status` empty, workspace `cargo build` green, `friday-mobile` **72 tests pass** on the restored state.

**Process guard going forward:** qualification must run `cargo fmt --check` (not mutate) on the workspace, or run `cargo fmt` scoped to the slice's crates, so we do not pick up unrelated formatting drift in every subsequent freeze.

---

## Scope guard for R5-2-5-2 (next slice)

- Modify only the daemon's command-ingress handler to add an explicit context-request command that composes `collector → ContextGate → EgressGate → provider` for **Screen**.
- Reuse existing `CommandGateway`, `WsServer`, `state.json` writer, `compose()`. Add **no** new IPC framework, **no** shell chooser, **no** ambient capture.
- Keep `services/context-collector`, `services/intelligence`, and `services/mobile`'s non-daemon code **untouched** unless a frozen interface must change (and if so, that change gets its own slice).

Next stones: **R5-2-5-2** (daemon integration) → qualification → freeze → **R5-2-5-3** (shell context chooser) → **R5-2-5-4** (Linux/Omarchy real capture qualification) → **R5-2-5** frozen.

> Note: the Omarchy standalone ISO/packaging work remains **separate** from FRIDAY's OS-facing nervous system. Packaging is deferred until this layer has real functionality worth shipping as a product.
