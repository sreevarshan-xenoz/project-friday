# Release Process

How work flows from idea to shipped, given the current maturity.

## Maturity (as of this writing)

| Phase                   | Status         |
| ----------------------- | -------------- |
| Architecture            | ✅ Complete     |
| Infrastructure          | ✅ Complete     |
| Cognitive Platform      | ✅ Complete     |
| Reliability Engineering | 🟡 In Progress |
| Product                 | 🔵 Not Yet     |
| Desktop Assistant       | 🔵 Not Yet     |

## Release gates (measurable)

A change may merge only if **all** hold. These are objective and checked in
review — none are "looks good to me":

1. **CI green** — `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all -- --check`, and `cargo test --workspace` all pass.
2. **No benchmark regression** — the `benchmarks/<date>.json` diff shows no
   regression per `benchmark_policy.md` (Raw MTBF, coverage, qualification bar).
3. **Specs precede implementation** — any public-API or behaviour change updates
   `specs/` (frozen `docs/` only via authorized addendum) before code merges.
4. **New capability carries evidence** — a new subsystem is not merged without
   benchmarks + chaos scenarios (`benchmark_policy.md`) and, before user exposure,
   a Certification path (`certification.md`).

## Discipline

- The foundation is coherent and layered; do not grow faster than the ability to
  validate. Reject feature work lacking benchmark or reliability justification.
- Keep changes small and CI-green. Stage narrowly; do not fold unrelated work
  (e.g. M5-intelligence) into reliability commits.
- Prefer repeating the unglamorous loop — increase subsystem qualification, reduce
  failure rates, improve recovery, certify capabilities, collect evidence — over
  adding new surface area.

## Documentation over crates

If FRIDAY becomes multi-developer, this `engineering/` handbook (and `docs/`,
`specs/`) will matter more than another crate. Maintain it as the project grows.
