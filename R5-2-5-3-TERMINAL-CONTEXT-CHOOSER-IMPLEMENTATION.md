# R5-2-5-3 — Terminal Reference Context Chooser (IMPLEMENTATION FREEZE)

**Status:** FROZEN implementation of the R5-2-5-3 shell context chooser — a terminal-based, consent-first Screen share reader that reuses the frozen R5-2-5-2 `context.read` seam.
**Inputs:** frozen R5-2-5-3-0 survey (`3a49a98`), frozen R5-2-5-2 (`26a0e41`), frozen collector (`e3cdba7`), frozen gate (`19ec6b8`), `dev @ 3a49a98`.
**This freeze:** adds `friday-ctl context screen`, a minimal consent + disclosure reader. No QML, no Omarchy modifications, no new IPC, no provider changes, no ambient/automatic capture.

---

## What was built — all in `services/mobile/src/bin/friday-ctl.rs`

| Symbol | Contract |
|---|---|
| `screen_disclosure()` | Pulls the authoritative Screen disclosure from the frozen `ContextGate` (`disclosure_for(Screen, Subscription)`), so the client never duplicates the system-of-record wording. Returns a meaningful non-empty string. |
| `confirm_context_consent()` | The reference consent UX: explains **what** would be shared (a screen capture still image) and **to whom** (the `FRIDAY_CONTEXT_PROVIDER`, default `fabric`), shows the **data-use** disclosure, then demands an explicit `y`. Any decline/error → `false`. |
| `run_context(addr, token)` | Sends `context.read` with a per-use `{ token, "consent": true }` — only reachable after the user answered `y`. |
| `run()` (refactored) | `goal` / `system-info` arms unchanged, now delegate to the shared `send_command` helper instead of inlining the connect/send/await loop. |
| `send_command(addr, method, params)` | Extracted connect → send one command → await the matching `CommandResult` loop, shared by all subcommands. |

### Consent ordering (the security-critical property)
`main()` dispatches `context screen` **before** any daemon or state-file access:
1. `context` (bare) or `context <non-screen>` → rejected, nothing shown, nothing sent.
2. `context screen` → `confirm_context_consent()` runs first. If the user declines (`n`, anything but `y`, or an input error): prints *"Aborted — nothing was shared; the provider was never contacted."* and exits `0` **without touching the daemon**.
3. Only after `y` does token resolution + the WebSocket send happen.

This means the provider is guaranteed to see nothing unless the user explicitly consents — the terminal mirror of the invariant already proven at the provider seam in R5-2-5-2.

### Pre-existing lint resolved (R5-2-5-3-owned)
- The `clippy::collapsible_match` / single-pattern-destructure warning at the old `friday-ctl.rs:164` (flagged in R5-2-5-2 as belonging to this slice) is resolved: the nested `decode_server` → `CommandResult` match is collapsed into `if let ... = decode_server(...)`. This unblocks `cargo clippy --bin friday-ctl -- -D warnings`.

---

## Verification

| Gate | Result |
|---|---|
| `cargo clippy -p friday-mobile --bin friday-ctl -- -D warnings` | **Clean** (this file's gate now passes). |
| `cargo clippy -p friday-mobile --lib -- -D warnings` | Clean (unchanged). |
| `cargo test -p friday-mobile` | **79 lib** + **1 integration** pass (no regression from the `run`/`send_command` refactor). |
| `cargo build` (workspace) | Clean. |
| Manual `context screen` (decline, `n`) | Prints share/disclosure, then *"Aborted — nothing was shared; the provider was never contacted."*, exit `0`, **no daemon access**. |
| Manual `context screen` (accept, `y`) | Prints share/disclosure, then reaches token resolution / connect (correctly reports "daemon not running" on the Windows dev box, since no Omarchy daemon is up). |
| Manual `context` (no channel) / `context foo` | Rejected with *"only 'screen' context is supported"*. |

**fmt note:** only `friday-ctl.rs` was edited, and only with targeted string edits — **no `cargo fmt`** was run on this file or the workspace, preserving the committed formatting and honoring the documented `add7219` contamination guard.

---

## Scope guard

This freeze touches **exactly one file**: `services/mobile/src/bin/friday-ctl.rs`.

It does **not** touch: any QML, any Omarchy shell/plugin/config, any `IpcHandler`, the daemon, the collector, ContextGate, EgressGate, providers, or the protocol. No new IPC. No autonomy.

**Out of scope (deferred):**
- Hotkey → `omarchy-shell` → `friday.shell` panel chooser (**R5-2-5-3-1**; reuses this proven core as a UI over it).
- Selection / App / Clipboard channels (future slivers beyond Screen).
- Linux/Omarchy real capture + vision provider qualification (**R5-2-5-4**).
- ChatGPT-web / CDP, ambient/automatic context.

**Known follow-up:** the disclosure uses `ProviderTarget::Subscription` (a remote class) so `disclosure_for` returns non-empty text for *any* remote provider; the provider name is surfaced from `FRIDAY_CONTEXT_PROVIDER` (default `fabric`), matching the daemon's own startup default.

Next stones: **R5-2-5-3-1** (Omarchy panel chooser over this core) → **R5-2-5-4** (Omarchy real-capture + vision qualification) → R5-2-5 frozen.
