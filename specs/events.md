# Events Specification (locked)

The event vocabulary is the contract every subsystem speaks. It is **frozen**: do
not edit event names or payloads. New subsystems consume and produce these events;
they do not invent parallel vocabularies.

## Vocabulary

Defined in `friday-planner/src/events.rs`. All events are bus messages.

```
goal.raw            — raw user input arrives (produced by UI/ingress)
intent.created       — Intent engine classified the goal into an intent
plan.created         — Planning engine produced a TaskGraph
task.started         — a task node began execution
task.completed       — a task node finished successfully
task.failed          — a task node failed
task.retry           — a task node is being retried
exec.action          — kernel dispatches an action to a service
exec.result          — service returned an ActionResult
verification.started — Verification engine began checking a result
verification.passed  — result satisfied the goal/constraints
verification.failed  — result did not satisfy the goal/constraints
```

## Rules

- A subsystem must declare, in its spec, which events it **consumes** and which it
  **produces**.
- `exec.action` / `exec.result` are the only execution boundary events; everything
  between `plan.created` and `task.*` is planner-internal.
- `task.failed` must be followed by `task.retry` or `task.completed`/`task.failed`
  terminal; the kernel owns retry policy (`DEFAULT_MAX_RETRIES`).

## Event envelope (deferred, non-breaking)

Rather than retrofitting `execution_id` / `trace_id` onto every frozen type, future
work wraps events in:

```
EventEnvelope<T> {
    event_id: String,
    execution_id: String,
    trace_id: String,
    timestamp: DateTime<Utc>,
    payload: T,
}
```

The inner payload schema is unchanged; the envelope adds distributed tracing,
replay, and correlation. This is the preferred path over editing locked event
types, and is how the Simulation `report` (M4.7) and Context Builder (M4.10) attach
correlation without breaking the contract.

## Goal lifecycle events & state.json

The daemon publishes goal lifecycle events (`goal.started`, `goal.progress`, `goal.completed`,
`goal.failed`, `goal.cancelled`, `goal.paused`, `goal.recoverable`) and reflects the finished
goal in `state.json` (`last_goal`).

### `served_by` attribution (optional)

When a goal completes with an answer served by an AI provider/model, the `goal.completed`
event payload and `last_goal` in `state.json` carry an optional `served_by` object:

```json
"served_by": {
  "provider": "opencode-zen",
  "model": "inclusionai/ling-3.0-flash:free",
  "ms": 2300
}
```

- `provider`: provider identifier string (e.g. `opencode-zen`, `kilo`, `claude-code`, `codex`, `antigravity`, `cline`, `llamacpp`)
- `model`: vendor model identifier string
- `ms`: total answer elapsed time in milliseconds

Absent when unknown (heuristic answer, local action). Never includes keys or URLs with secrets.

### `masked` secret count (optional)

When one or more secrets (private keys, bearer tokens, passwords in connection strings,
credential assignments, or well-known API keys) are masked in the goal text before
sending to the AI provider, the `goal.completed` and `goal.failed` event payloads and
`last_goal` in `state.json` carry an optional `masked` count:

```json
"masked": 1
```

- `masked`: integer count of secrets masked before planning and model inference

Absent when no secrets were masked (count is 0). Raw secrets are replaced with
`[secret masked]` and never leave the machine, appear in logs, or persist in state.json.

### `private` mode flag

The root of `state.json` carries a boolean `private` field (`true` | `false`) indicating
whether FRIDAY is operating in private mode (`FRIDAY_PRIVATE=1|true`). When `true`, FRIDAY
uses local models exclusively and answers stay on the machine.

```json
"private": true
```

## Egress Log: egress.jsonl

The daemon maintains a local audit log of outbound AI requests at `<state dir>/egress.jsonl`
(`~/.local/state/friday/egress.jsonl` on Linux, `%LOCALAPPDATA%\friday\egress.jsonl` on Windows,
or `FRIDAY_EGRESS_FILE`), allowing the person to see exactly which AI provider received
what kind of data from each ask — never the content itself.

### Privacy Guarantee
`egress.jsonl` records metadata and counts only. It **NEVER** records:
- The prompt, question text, or selected text
- The model answer or output text
- API keys, credentials, or secrets
- URLs with query parameters or secrets
- Any raw user or system content

### Fields
Each line is a JSON object appended whenever a request attempt is made to a provider
(including fallbacks and failures):

```json
{
  "ts": "2026-11-14T18:00:00Z",
  "goal_id": "goal-b1a9c3d4",
  "provider": "opencode",
  "model": "inclusionai/ling-3.0-flash:free",
  "kinds": ["text"],
  "text_chars": 1240,
  "image_bytes": 0,
  "masked": 0,
  "outcome": "ok"
}
```

- `ts`: RFC 3339 UTC timestamp of the attempt
- `goal_id`: goal identifier string if known (omitted when unknown)
- `provider`: provider identifier string (e.g. `opencode`, `kilo`, `claude-code`, `codex`, `antigravity`, `cline`, `ollama`, `llamacpp`, or search backends `searxng`, `brave`, `duckduckgo`, `wikipedia`)
- `model`: vendor model identifier string
- `kinds`: array of payload content kinds: `["text"]`, `["text", "image"]`, or `["search"]`
- `text_chars`: count of text characters sent
- `image_bytes`: count of image bytes sent (0 if none)
- `masked`: secrets masked in that ask before transmission (integer count if known, omitted when unknown)
- `outcome`: `"ok"` if the attempt succeeded, `"error"` if it failed or timed out
- `local`: `true` for local providers (llama.cpp/Ollama on localhost, or local SearXNG); omitted for cloud providers

Agent CLI providers (`claude-code`, `codex`, `antigravity`, `cline`) count as sent.

### Bounding and Permissions
- **File size bounds**: The file is bounded to ~1 MB (`DEFAULT_MAX_EGRESS_BYTES = 1,048,576`). When it exceeds this bound, the newest half of the lines is kept and atomically written.
- **File mode**: Created with mode `0600` (`-rw-------`) on Unix so only the owning user can read it.
- **Non-failing writes**: Writing to `egress.jsonl` must never fail an ask (any I/O error is logged as a warning and execution continues).

### `web` search request flag (optional)

On goal ingress commands (`planner.goal`) and bus intake events (`plan.goal`), an optional
boolean `web` requests web search augmentation:

```json
{
  "goal": "what is the latest stable Rust version",
  "web": true
}
```

- `web`: boolean (`true` queries an ordered chain of search backends — SearXNG, Brave Search API, DuckDuckGo HTML, Wikipedia API — uses the first returning results, injects them into the prompt as numbered sources with citation instructions, and appends a verified `Sources (<Backend>):` list to the final answer; absent or `false` leaves web search disabled).
- Privacy: web searches happen ONLY on explicit user request (`friday ask --web`, `friday-ctl goal --web`, or explicit prefixes like "search " / "search the web for " / "look up "). Never automatically. Egress log records provider as the backend contacted with `kinds: ["search"]`, one entry per attempted backend.

### `thorough` and `avoid` goal fields (optional)

On the same goal ingress commands and `plan.goal` events, an optional boolean `thorough` asks for a second, more careful answer (the answer pill's "Try another model"), and an optional object `avoid` names the model that gave the answer being retried:

```json
{ "goal": "explain E0308", "thorough": true, "avoid": { "provider": "opencode", "model": "big-pickle" } }
```

- `thorough`: the answer step ranks models for quality (a connected account, then a provider with the person's own key, then free reasoning-tier models, then the rest), puts a take-more-care note first in the prompt, and routes by the same privacy rules as any ask (private mode stays local).
- `avoid`: `{provider, model}`, both required. That model is tried last, never excluded, so a single-model setup still answers. FRIDAY has no model quality ratings, so "another model" is the honest claim; the order is a preference, not a measurement.



