# Design Study: Cloudflare Computer (`@cloudflare/computer`)

**Status:** Design Study — captured for decision record, not an ADR.
**Date:** 2026-08-08
**Source:** https://github.com/cloudflare/computer (MIT, TypeScript, preview release)
**Reviewed:** 2026-08-07 (commit ~74ef2ed, docs/01–19)

> A design study is a record of *why* we adopted or rejected a pattern from an
> external system. It is not a decision — that happens in `docs/adr/`. If a
> pattern is adopted, open an ADR and link it from here.

## TL;DR

Cloudflare Computer is an **agent execution runtime**: a SQLite-backed virtual
filesystem inside a Durable Object, with one pluggable execution router
(`workspace.runtime`) over three backends (Linux container via FUSE, just-bash
Dynamic Worker, isolated JS module Worker).

It is **not** a computer-use / desktop-control / GUI-automation framework. There
is no mouse, keyboard, or screen abstraction. The earlier assumption that this
was FRIDAY's future "Computer Organ" is incorrect and has been discarded.

The genuinely valuable material for FRIDAY is **not the substrate** (DO, SQLite
VFS, FUSE, capnweb, Workers — all Cloudflare-lock-in). It is a set of execution
semantics:

1. Separation of **command execution** from **state synchronization**
   (the "sync bracket" with durable retry — never rerun the command).
2. A **streaming exec handle** (events + buffered result).
3. A **readonly safety tier** in the tool layer.
4. **"Exec output is untrusted text"** as an explicit convention.
5. **Model-facing backend descriptions** so the model picks the cheapest
   execution path.

## Problem it solves

Coding agents need a working directory, a shell, and tools. Cloudflare's answer:
give every agent a **durable virtual filesystem** (authoritative state in SQLite
inside a Durable Object) and a **pluggable execution surface** that can route a
command to whatever compute primitive is appropriate — a full Linux container for
native binaries, or a fast isolate for file manipulation. All operations are
gated, audited, and observed.

The core engineering problem they attack is: **commands have side effects and
state lives on the host.** If execution and synchronization are the same
operation, an unknown state after a crash forces a rerun — harmless for
idempotent commands, catastrophic for ones that are not.

## How it works (architecture)

```
Durable Object (authoritative)
   ├── VFS  (SQLite)           fs: readFile/writeFile/mkdir/readdir/grep/rm
   ├── runtime                 exec/getExec/killExec/disposeExec
   ├── git                     isomorphic-git glue over the workspace tree
   ├── assets                  R2 share + presigned URL
   └── artifacts               Git-compatible versioned storage facade

workspace.runtime backends (routing, not authorization):
   ├── container-shell      Linux userland via FUSE mount + computerd daemon
   ├── worker-shell         just-bash in a Dynamic Worker (shares host store)
   └── worker-javascript    isolated ESM modules, durable node:fs/promises
```

### The sync bracket (the key idea)

Command backends synchronize around every execution:

```
push  →  spawn  →  stream events  →  result  →  pull
```

- `push`: flush DO-side writes into the backend's view of the filesystem.
- `spawn`/`result`: run the command, buffer output.
- `pull`: reconcile backend-side writes back into the authoritative store.

If the command completes but the post-command `pull` fails, the result reports
`sync.status = "pending"` and a `SyncRetryScheduler` durably retries the pull —
**without rerunning the command**. Module backends (`worker-javascript`) share
the host store and skip the bracket entirely (no push/pull round trip).

### Execution handle

```ts
exec(source, { backend, cwd, timeoutMs, env }) → Handle
  handle: ReadableStream<Event> + result() + kill()
```

The handle is single-consumer: read the stream **or** call `result()`, not both.
Events (`stdout`/`stderr`/`exit`) pipe directly into an SSE stream for live
output in a browser. `worker-javascript` keeps a durable execution journal in the
Workspace database — replay, cancellation, disposal, and restart recovery
(orphaned running records reconcile to failed). `worker-shell` deliberately
preserves one-call, buffered-result behavior; no reattachment.

### Tool layer

`@cloudflare/computer/tools` ships AI-SDK tools: `read`, `write`, `edit`, `ls`,
and opt-in `exec`, `publish`.

- `readonly: true` collapses the set to `read` + `ls` — no mutation, no exec.
- `exec` appears only when explicitly opted in.
- `edit` matches against the **original** file (not incrementally), rejects
  overlapping/nested edits, preserves file mode, normalizes line endings,
  returns a unified patch.
- Each shell backend carries a **model-facing description**; the model chooses
  which backend a command runs on.

## Patterns worth adopting

| # | Pattern | Why it's valuable | FRIDAY mapping |
|---|---------|-------------------|----------------|
| 1 | **Execution/sync separation** (`push → spawn → result → pull`, durable retry of `pull` only) | Fixes the "unknown state after crash" problem without risking a non-idempotent rerun | `planner/src/execution/executor.rs` + `scheduler.rs`; the action runtime (`services/action-runtime`) |
| 2 | **Streaming exec handle → SSE** | One interface for live output across terminal, docker, ssh, python, git, minecraft | Action-runtime execution result/streaming surface |
| 3 | **Readonly safety tier** | "Review/index/support" agents shouldn't be able to mutate or exec; no per-action permission spam | `Risk` model in `friday-capabilities::spec` + `PermissionEngine` (`services/action-runtime/src/permission.rs`) |
| 4 | **Exec output is untrusted text** | Tool output must never be fed back as trusted instructions | Context builder (`services/context/src/lib.rs`) + planner verification |
| 5 | **Backend selection is routing, not authorization** | Model chooses the cheapest backend; the gateway validates against a host-owned allowlist | Existing provider routing doctrine in `services/intelligence` |
| 6 | **Model-facing backend descriptions** | The model needs plain-language descriptions to route cheaply | `CapabilityRegistry::search` ranking (`planner/src/execution/capability_registry.rs`) |
| 7 | **Durable execution journal** (replay/cancel/dispose/restart-reconcile) | Validates FRIDAY's existing design rather than adding new work | `planner/src/experience/timeline.rs`, scheduler chaos phase |

### Adoptable as a concrete FRIDAY change (when Execution Layer work begins)

The #1 pattern becomes a candidate ADR: **the action runtime records an
execution as two phases** — (a) spawn + result, (b) side-effect reconciliation —
and only (b) is retried durably after a crash. This is a real change to how
`services/action-runtime` reports command outcomes, and it is the single highest
value idea in this repository.

## Patterns rejected

| Pattern | Why rejected |
|---------|--------------|
| Durable Object as authoritative store | Cloudflare-lock-in; FRIDAY is local-first (Windows/Linux host), state already lives on disk |
| SQLite VFS substrate | Same reason; FRIDAY's filesystem *is* the real filesystem |
| FUSE mount projection | Requires container sandboxes; FRIDAY executes natively |
| capnweb RPC wire protocol | Locked to the DO↔container sync problem FRIDAY doesn't have |
| Isolate-vs-container dynamic orchestration | Irrelevant on a machine with real processes; FRIDAY's "container" story is Docker-as-a-tool, not execution substrate |
| `worker-javascript` durable-module execution | Interesting but a distant future; the existing `terminal.*` path covers today's needs |
| ~10GB / in-memory container FS constraints | Follow from the substrate; not applicable |

## What it is *not*

- Not a computer-use / GUI-automation framework (no mouse/keyboard/screen).
- Not a cognitive architecture — no planner, memory, or skill model.
- Not production-ready (explicit "PREVIEW ONLY" warning; APIs unstable).

## How it maps to FRIDAY

```
FRIDAY
  Brain
  Planner            ← reads plan from skills/capabilities, not from this study
  Memory
  Skill Engine
  Capability Registry   ← FRIDAY's equivalent of "the tool layer"
  Execution Layer       ← Cloudflare's sync-bracket + streaming-handle ideas land HERE
    providers
  Desktop Organ
  Voice Organ
  Vision Organ
```

The classification that fits FRIDAY's architecture: Cloudflare Computer is a
**Remote Capability Host / Execution Runtime** design. It is a reference for how
FRIDAY's Execution Layer should behave, not a component FRIDAY should embed.

## Future applicability

If FRIDAY ever distributes (laptop → edge → cloud → container), this study
becomes the seed for a **remote-execution capability**: a capability id like
`remote.exec` whose backing service owns the sync bracket, and where FRIDAY's
local Execution Layer and the remote host agree on "routing is not
authorization." Revisit this study then, not now.

## Related notes

- The **untrusted-tool-output** convention should inform planner context
  assembly even before any Execution Layer work: tool results are data, never
  instructions.
- The **readonly tier** can ship immediately as a skill-manifest field
  (`readonly: true` on a skill → planner auto-approves only `Risk::Safe`
  capabilities and never proposes `exec`-class actions) — see
  `services/action-runtime/src/permission.rs` and `friday-capabilities::spec`.

## Status of decisions

- [ ] ADR: action-runtime two-phase execution (spawn/result vs. side-effect
      reconciliation) — **proposed** when Execution Layer work begins.
- [ ] Skill manifest `readonly` field — candidate for the Skill Engine
      implementation (`specs/skill-engine.md`). Concrete shape, when built:

      ```json
      {
        "id": "git",
        "readonly": true,
        "permissions": ["filesystem.read"]
      }
      ```

      `readonly: true` → the Skill Engine instructs the Permission Engine to
      auto-approve only `Risk::Safe` capabilities and never propose
      `exec`-class actions for this skill's plans. Anything outside that set
      (`filesystem.delete`, `terminal.execute`) still prompts the user.

## Revisit Trigger

Re-evaluate this study when:

- The Execution Layer work begins (the two-phase execution ADR is proposed).
- Remote execution is added (`remote.exec` capability).
- Distributed workers / edge deployment are introduced.
- Skill sandboxing changes (containers become an execution substrate rather
  than a tool).
- A GUI/desktop-control project is studied and changes the Computer Organ
  picture.
