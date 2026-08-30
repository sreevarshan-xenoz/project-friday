# R5-1-3 Survey — Desktop → FRIDAY action

Status: **survey only — no code yet.** This traces the existing R5-1-1 command
path end-to-end against the real code, records each seam, and freezes the
acceptance boundary before any implementation. It deliberately does not invent
architecture: every link below is verified against the committed tree at
`13136ee` .. `be04df0`.

## The acceptance target (brutally simple)

> **One user action in the desktop causes one real FRIDAY goal to execute, and
> the resulting truth comes back to the desktop.**

Scoped out of R5-1-3 (they belong to R5-2): ChatGPT session, Google login,
headless browser, "FRIDAY controls everything."

## The verified command path

```
friday.shell (desktop action)
   │  user clicks "GO" on a goal  [MISSING — R5-1-2 removed command input]
   ▼
friday-ctl goal "<text>"
   │  reads token from state.json, connects loopback WS (127.0.0.1:18432)
   ▼
WsServer ingress  (token-gated)
   │  transport.rs:307 handles ClientMessage::Command
   │  transport.rs:314 rebinds session_id to the live connection
   ▼
CommandGateway.dispatch   (commands.rs:41)
   │  planner.goal → publishes bus Event("plan.goal", session_id)
   │  returns {accepted:true} immediately (async by design)
   ▼
GoalRunner.run_goal   (subscribed to plan.goal)
   │  real Executive.cycle over real evidence adapters
   ▼
Executive phase cycle → report
   ▼
bus events: goal.started → goal.completed|failed|cancelled|paused|recoverable
   ▼
shell_state.apply_event → state.json   (hardened in R5-1-2)
   ▼
friday.shell FileView → FridayState.qml → calm panel
```

**Every link below `friday-ctl` is already built, wired, and proven.** The
single thing R5-1-3 must add is the **top link**: an intentional command
surface in the widget. R5-1-2 deliberately removed it, so R5-1-3 restores it —
but as a *deliberate, user-initiated* action, not an ambient command channel.

## Seam inventory

### Already present (proven, no work)
| Seam | Location | Proof |
|---|---|---|
| token-gated loopback WS ingress | `friday-linux-daemon.rs:270-305` | contract test `shell_contract.rs` (close token rejected) |
| `planner.goal` → bus event | `commands.rs:44-62` | unit test `planner_goal_publishes_session_id_on_plan_event` |
| GoalRunner → real Executive | `goal_runner.rs:1450-1457` | live run: `goal.completed`, phases 10, outcome=completed |
| terminal truth → `state.json` | `shell_state.rs` `apply_event` | live run: `last_goal.{outcome,phases,strategy}` |
| widget reflects truth | `FridayState.qml` FileView | R5-1-2 acceptance (deferred visual execution) |

### To build in R5-1-3
| Seam | What | Design constraint |
|---|---|---|
| Widget command action | A `GO` control in the panel that the user must press to send | Intentional, one-shot, not ambient |
| Widget → `friday-ctl` | `root.bar.run("friday-ctl goal ...")` (the Omarchy launch seam from `agents` Panel) | No shell injection; use the existing launch seam |
| Token authority | `friday-ctl` reads the live token from `state.json` (`shell_state.default_path`) | Never stored elsewhere; refreshed per send |

## Key architectural truths (why R5-1-3 stays small)

1. **The goal is async by design.** `planner.goal` returns `{accepted:true}`
   immediately; the *result* arrives later via `state.json` (goal terminal
   state) which the FileView widget already watches. R5-1-3 does **not** need a
   synchronous request/response round-trip or a live WS event stream on the
   desktop.
2. **No BusBridge on the Linux path — and that's fine.** The mobile daemon
   republishes bus events over WS for phones; the Linux desktop uses the
   file-backed `state.json` read-model instead, which is the calmer, contract-
   clean choice the R5-1-1 integration contract already froze. Adding a Linux
   BusBridge would be scope creep for R5-1-3.
3. **`session_id` is rebound per-connection** (transport.rs:314), so
   `friday-ctl` need not supply one; S1 per-session continuity is handled by
   the transport layer, and GoalRunner keys on it.
4. **Result truth that exists** (genuine): goal active/working, terminal
   outcome, phases completed, strategy/route, verification evidence, result or
   error, CPU/mem. R5-1-2 proved these flow. No fabricated "Planning 73%".

## Acceptance boundary (frozen before code)

Given the async design, R5-1-3 proves:

- [ ] User presses the widget's `GO` once (a real intentional action) → `friday-ctl` runs `planner.goal` with the live token.
- [ ] `friday-ctl` (or an in-process call) returns `{accepted:true}` and the goal begins.
- [ ] `state.json` transitions `current` → active (goal genuinely in flight).
- [ ] The goal executes a real Executive cycle (web/heuristic evidence).
- [ ] `state.json` lands on a true terminal (`goal.completed|failed|...`) with genuine outcome detail.
- [ ] The widget's FileView picks up the terminal state without a restart (state refreshes live).
- [ ] Regression: R5-1-1 contract test + R5-1-2 unit suite stay green; `omarchy/` untouched; zero new warnings; no new runtime dependency on Omarchy internals.

The visual on-screen verification (real click in a live Omarchy session)
remains gated on the visual-verification step, as before.

## Hard seam: what NOT to build

- No headless browser / no web-session provider (R5-2).
- No live WS event stream to the desktop unless a future slice needs it (R5-1-2 chose file-backed; preserve that).
- No free-text "FRIDAY, do everything" — commands stay the structured `method` set (`planner.goal`, `task.*`, `system.info`) from `commands.rs:21-27`.
- No change to frozen `omarchy/`.

## Note for R5-2-0 (separate thread, not today)

R5-1-3 is the OS nervous system. The AI-provider capability survey (R5-2-0) is
a separate research thread: it maps which *capabilities* FRIDAY can use
(local engines, web sessions, API keys) **before** any provider design. The
ChatGPT-web idea lands there strictly as a research question with technical and
policy boundaries (a user-controlled browser session ≠ an official API), after
this nervous system is solid.
