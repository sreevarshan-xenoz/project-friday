# Intelligence Specification (M5)

**Status:** ⏳ planned. **Spec-first:** implement against this file.

The Intelligence Layer is the **LLM**, and it sits **above the kernel**, not inside
it. It is fed by the Context Builder and reasons over Capability Intelligence. It
never sees plugin internals and never assembles its own context.

## Crate

`services/intelligence` — crate `friday-intelligence`. Depends on
`friday-context` (M4.10), `friday-capabilities`, `friday-planner`.

## Position in the stack

```
User → Context Builder → Intelligence Layer → Planner → Simulation → Kernel
```

The Intelligence Layer receives **only** an `LLMContext` (from `specs/context.md`)
plus the goal. It does **not** query memory, the registry, or services directly.

## Provider abstraction

- Local model via **Ollama** + cloud fallback (configurable).
- Structured output parsing into the planner's `TaskGraph` / `intent` shapes.
- Tool selection is done by **querying Capability Intelligence**, not by hardcoding
  plugin names.

## What it replaces (behind the same contract)

At M5 the rule-based **Intent** and **Planning** engines are replaced by
model-driven ones — but they still emit the **same events** (`intent.created`,
`plan.created`) and produce the **same `TaskGraph`**. Nothing downstream (Simulation,
Kernel, Services) changes. This is why the event vocabulary and `TaskGraph` are
frozen.

## Consumes / produces

- Consumes: `LLMContext` (from Context Builder), `goal.raw`.
- Produces: `intent.created`, `plan.created` (into the existing Planner pipeline),
  ultimately `exec.action` via the kernel.

## Constraints

- The LLM never assembles context (Context Builder owns that).
- The LLM never calls plugins directly (it emits intents/plans; the kernel dispatches).
- All model output is validated by the Simulator (M4.7) before execution.

## See also

- `specs/context.md` (feeds this layer).
- `specs/capabilities.md` (tool/provider selection).
- `specs/planner.md` (same event contract preserved).
- `M5` in `docs/Roadmap.md`.
