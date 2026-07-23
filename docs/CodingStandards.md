# Coding Standards

We hold the codebase to a single bar: clean, typed, formatted, and tested.
CI enforces every rule below.

## Branching

- `main` — always releasable.
- `dev` — integration branch.
- `feature/*` — one logical change per branch, branched from `dev`.

Never develop on `main`.

## Rust (`core`, `planner`, `services/*`, `mcp`, `plugins/*`)

- Format: `cargo fmt --all`
- Lint (warnings are errors): `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test` (prefer targeted `-p <crate>`; full workspace when needed)
- No `unsafe` unless reviewed and documented.

## C# (`windows-host`)

- Target: .NET 8.
- Keep the host a pure capability executor — no planner / LLM logic.

## Kotlin / Android (`clients/android`)

- Clients implement the Mobile Protocol only; no FRIDAY business logic on-device.
- Qualification gates: `specs/client-qualification.md`.

## Python (future vision / voice / plugins)

- Package manager: **uv** (avoid bare `pip`).
- Format: `black .`
- Lint: `ruff check .`
- Types: `mypy .`
- Tests: `pytest`
- Hooks run via `pre-commit` when Python trees land.

## Commits

- Imperative subject, <= 72 chars: `add event bus router loop`.
- Reference issues/PRs where relevant.
- Never commit secrets, `.env`, or credentials.
- During **Alpha Qualification**, prefer PRs with benchmarks or measurable
  reliability gains over feature-only changes (`docs/Roadmap.md`).

## Pre-commit

A root `.pre-commit-config.yaml` wires project hooks. Install with:

```powershell
uv tool install pre-commit
pre-commit install
```

## Documentation

- **New subsystems:** lock a spec under `specs/` *before* implementation.
- Update `docs/Modules.md` and the relevant design doc / README when status
  changes.
- Public Rust items carry doc comments; crates build with `cargo doc`.
- `docs/` is design history + narrative; `specs/` is the contract source of truth
  for post-M4.7 work.
