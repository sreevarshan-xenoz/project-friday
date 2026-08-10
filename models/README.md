# models/

Model specialist registry, routing, and provider configuration.

**Implementation lives primarily in `services/intelligence`**
(`friday-intelligence`):

- Provider abstraction (`Provider` / `ProviderV2`, OpenAI-compatible, OmniRoute,
  Ollama, llama.cpp, Gemini, OpenGateway, mocks)
- `InferenceFabric` + circuit breakers + **per-attempt `request_timeout`**
- **Capability-based routing** (`RoutingMode::Local`/`Hybrid` by
  `ModelCapability`, no provider-name leaks; frozen `RoutingDecision` v1)
- **Immutable `QualificationProfile`** (TTFT, tokens/s, JSON/tool accuracy, RAM/VRAM)
  written only by `provider-qual`; fabric derives `routing_score(profile, health)`
- **Health telemetry** — `[Provider Health]` log + `.fabric/health-changes.jsonl`
  (recovered/degraded/demoted/probing), sanitized `load_health`, atomic `persist_health`
- `IntelligenceRouter` (cognitive mode / System 1–2 style routing)
- `PromptCompiler` and `ResponseInterpreter`
- Planner backend hooks (`InferenceBackend`)

Local-first intent (Ollama / llama.cpp on the reference RTX 3060 machine) with
optional cloud fallback remains the product direction; concrete local wiring
continues to grow behind the same provider traits.

## Status

✅ Core intelligence / provider fabric implemented (M5) — qualification profiles,
health telemetry, and the **chaos laboratory** (`provider-qual --chaos`, 18
scenarios covering provider failures + health-persistence corruption) shipped
(Phases 1+2).  
⏳ Scoring-model validation (replay yesterday's requests against alternate
routing functions), a week of health telemetry before hysteresis, and the full
specialist catalog (vision, speech, dedicated coding models) remain open.

See `specs/intelligence.md`, `specs/inference_fabric.md`, `specs/provider-engine.md`,
and `docs/Architecture.md`.
