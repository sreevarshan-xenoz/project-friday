# Coding Standards

We hold the codebase to a single bar: clean, typed, formatted, and tested.
CI enforces every rule below.

## Branching

- `main` — always releasable.
- `dev` — integration branch.
- `feature/*` — one logical change per branch, branched from `dev`.

Never develop on `main`.

## Rust (`core`, services, plugins in Rust)

- Format: `cargo fmt --all`
- Lint (warnings are errors): `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test --all`
- No `unsafe` unless reviewed and documented.

## Python (orchestrator, vision, plugins)

- Package manager: **uv** (avoid bare `pip`).
- Format: `black .`
- Lint: `ruff check .`
- Types: `mypy .`
- Tests: `pytest`
- Hooks run all of the above via `pre-commit`.

## Commits

- Imperative subject, <= 72 chars: `add event bus router loop`.
- Reference issues/PRs where relevant.
- Never commit secrets, `.env`, or credentials.

## Pre-commit

A root `.pre-commit-config.yaml` wires Rust and Python hooks. Install with:

```powershell
uv tool install pre-commit
pre-commit install
```

## Documentation

Every module gets documentation **before** code. New modules update
`docs/Modules.md` and `docs/PluginSDK.md`. Public Rust items carry doc comments;
the crate builds with `cargo doc`.
