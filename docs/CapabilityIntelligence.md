# Capability Intelligence (M4.6)

> **Status:** ✅ Implemented (`friday-capabilities` in `services/capabilities`).
> Design locked. Implements the "Knowledge" subsystem: FRIDAY learns
> **what it can do** and *how well*, so the planner reasons over capabilities and
> providers instead of hardcoded plugin names. Supersedes the earlier
> "Capability Registry" name. Companion to `docs/WindowsBridge.md` and
> `specs/capabilities.md`.
>
> Terminology shift (authorized restructure): we now speak of **services** (a
> filesystem service, a git service, a windows service, a future browser service)
> rather than bare plugins, and of **subsystems** (Kernel / Knowledge / Services /
> Perception / Cognition / Autonomy) rather than a flat milestone list. See
> `docs/Architecture.md` addendum and `docs/Roadmap.md`.

## 1. Purpose

The planner must stop thinking `need file → filesystem` and start thinking
`need capability → registry → available providers → best provider`. This crate
is the single source of truth for *what the platform can do, who can do it, and
how reliable/expensive each option is*.

## 2. Capability model

Every service advertises rich `Capability` records (defined in
`friday-capabilities::model`):

| Field | Meaning |
| --- | --- |
| `id` | Fully-qualified capability, e.g. `filesystem.create_file` |
| `name` / `description` | Human + model-readable |
| `group` | Graph grouping, e.g. `Files`, `Windows`, `Browser` |
| `actions` | Verbs the service handles under this capability |
| `permissions` | Permission strings the provider needs |
| `platform` | `Windows` / `Linux` / `MacOS` / `Android` / `Cloud` / `Any` |
| `latency_ms` | Typical latency (for ranking) |
| `reliability` | 0..1 historical success rate |
| `version` | Provider/contract version |
| `dependencies` | Other services/capabilities required |
| `resource_requirements` | e.g. `display`, `network`, `admin` |
| `supports_parallel` | Can run concurrently |
| `dry_run` | Supports M4.7 dry-run |
| `available` | Runtime availability (negotiation) |
| `health` | `Healthy` / `Busy` / `Offline` / `Recovering` / `Disabled` |
| `cost` | Arbitrary cost units (compute/quota) |
| `confidence` | Registry confidence in this provider (0..1) |

## 3. Registry

`CapabilityRegistry` (in `friday-capabilities::registry`) stores
`provider → Vec<Capability>`. Operations:
- `register(provider, capability)` / `register_provider(provider, caps)`
- `deregister(provider)`
- `providers_for(capability_id)` → all providers claiming it
- `ranked_providers_for(capability_id)` → filtered to `available && health !=
  Offline/Disabled`, sorted by `ranking::score` (desc)
- `available(capability_id)` → any healthy provider?
- `set_health(provider, capability_id, health)` (negotiation write-back)
- `all()` / `capabilities()` / `graph()`

The registry is the planner's only view of the outside world; it never sees
service internals.

## 4. Provider ranking

`ranking::score(capability)` collapses the rich fields into a 0..100 number.

```
score = reliability * 100
      - latency_ms * 0.05
      - cost * 1.0
      + confidence * 2.0
```

Worked example (Browser vs Windows for `open_url`):

| Provider | reliability | latency | score |
| --- | --- | --- | --- |
| Browser | 0.99 | 20ms | **98** |
| Windows | 0.835 | 50ms | **81** |

Planner picks Browser — no hardcoding. Weights are centralized and tunable.

## 5. Runtime negotiation & health

`health::Monitor` periodically invokes a `HealthProbe` per (provider, capability),
writes the resulting `Health` back into the registry, and flips `available`. On
`Offline` it calls an optional **recovery** hook (e.g. ask the runtime to restart
the service) — the basis for self-healing. Health states drive autonomous
recovery: a crashed Git service is marked unhealthy, the planner routes around
it, and the runtime restarts it.

## 6. Manifest-based discovery

Services ship a `manifest.json` (`ServiceManifest`) alongside their code:

```json
{
  "service": "windows",
  "version": "1.0",
  "platform": "Windows",
  "capabilities": [ { "id": "windows.open", "name": "Open Application", ... } ]
}
```

`Manifest::load_dir(plugins_root)` scans `*/manifest.json`, **validates** each,
and returns the parsed services. Startup becomes scan → load → validate →
register. Adding/removing a service (or a capability like `browser.chrome.*`
when Chrome is installed) is data, not code.

## 7. Capability graph

`graph::CapabilityGraph` groups capabilities by `group` into a hierarchy
(`Files → Read/Write/Delete`, `Windows → Window/Clipboard/Process`, …). The LLM
traverses the graph instead of reading giant capability lists — large token
savings at M5.

## 8. This unlocks M5

```
LLM → need capability → registry → best provider → runtime → result
```

The LLM never sees implementation details. M4.7 (Dry Run) reads the `dry_run`
flag per capability; M4.8 (Workflow Recorder) reads the graph; the future
Browser service only declares `Navigate/Click/Type/Screenshot/ExtractText` and
the registry handles the rest.

## 9. Definition of Done

- ✅ Rich `Capability` model with all fields above.
- ✅ `CapabilityRegistry`: register/deregister/query/rank, availability filtering.
- ✅ Deterministic `ranking::score`; multi-provider ranking picks the best.
- ✅ `Health` states + `Monitor` with probes and self-healing recovery hook.
- ✅ `ServiceManifest` discovery (scan/validate/register) from `plugins/`.
- ✅ `CapabilityGraph` grouping for LLM traversal.
- ✅ Existing services (filesystem/terminal/git/windows) publish capabilities.
- ✅ Unit + integration tests (ranking example, health, manifest, graph, monitor).
