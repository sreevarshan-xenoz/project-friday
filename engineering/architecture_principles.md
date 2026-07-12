# Architecture Principles

This document records the *principles* that explain why the architecture in
`docs/Architecture.md` looks the way it does. It is complementary, not a duplicate:
the architecture describes the structure; these are the rules that guided it and
that new work must respect.

## Principles

1. **Core stays ignorant of implementations.** The core contains no intelligence;
   all behaviour lives in plugins/services. This keeps the core stable while
   capabilities evolve.

2. **Planner communicates only through events.** The reasoning engines exchange
   dot-namespaced events on the bus; they never call each other directly. This
   makes each stage independently debuggable, evaluable, and recoverable.

3. **Models are decoupled from providers.** The planner reasons over capabilities
   and ranked providers in the registry, never over provider/plugin names. Swapping
   a provider (e.g. Git CLI → libgit2) must not require planner changes.

4. **Recovery must never hide failure.** A recovered failure is still a system
   failure for measurement. Recovery is reported as a separate number, never folded
   into reliability. (See `recovery_policy.md`, `metrics.md`.)

5. **Benchmarks describe reality, not desired outcomes.** A benchmark is an
   honesty instrument; it is never tuned to produce a better score. (See
   `benchmark_policy.md`.)

6. **Specs precede implementation.** Behaviour and public APIs are written down in
   `specs/` before code. Frozen `docs/` change only via authorized addenda.

7. **Every new subsystem must justify itself with evidence.** No capability is
   merged without benchmarks + chaos scenarios, and none reaches users without a
   Certification path. (See `benchmark_policy.md`, `certification.md`.)

8. **No circular dependencies between crates.** Dependencies form a DAG; public
   APIs stay small and stable so the graph can evolve without churn.

9. **Every feature answers three questions.** Which event does it consume? Which
   event does it produce? Which plugin/service owns it? If you cannot answer, the
   design is not finished.

These principles are treated like the frozen architecture: stable until real
benchmark evidence gives a reason to change them.
