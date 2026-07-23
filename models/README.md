# models/

Model specialist registry, routing, and provider configuration.

**Implementation lives primarily in `services/intelligence`**
(`friday-intelligence`):

- Provider abstraction (`Provider` / `ProviderV2`, OpenAI-compatible, OmniRoute, mocks)
- `InferenceFabric` + circuit breakers
- `IntelligenceRouter` (cognitive mode / System 1–2 style routing)
- `PromptCompiler` and `ResponseInterpreter`
- Planner backend hooks (`InferenceBackend`)

Local-first intent (Ollama on the reference RTX 3060 machine) with optional cloud
fallback remains the product direction; concrete local wiring continues to grow
behind the same provider traits.

## Status

✅ Core intelligence / provider fabric implemented (M5).  
⏳ Full specialist catalog (vision, speech, dedicated coding models) and production
routing policies still expand with Vision / Voice milestones.

See `specs/intelligence.md`, `specs/inference_fabric.md`, and
`docs/Architecture.md`.
