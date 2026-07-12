# Benchmark Sprint — Reliability Harness

> Status: implemented (`services/benchmark`, crate `friday-benchmark`).
> Run: `CHAOS=1 cargo run -p friday-benchmark -- <n>` (default `n = 600`).

## Purpose

Architecture is done. The next job is to **prove the system is reliable** by
breaking it on purpose and measuring the numbers that matter. This harness is the
first instrument of Alpha Qualification.

## What it does

Generates a balanced corpus of `n` tasks across every subsystem — filesystem,
terminal, git, windows, inference, browser — and runs each through the
**Plan Simulation Engine** (Digital Twin). For filesystem tasks it also performs
the action for real in a throwaway temp dir, so we get a ground-truth
**Simulation Accuracy**.

## Metrics

| Metric | Definition | Target |
|---|---|---|
| **MTBF** | mean tasks between failures (a failure = the simulation could not confidently guarantee a safe, resolvable plan) | ↑ |
| **Simulation Accuracy** | predicted outcome == actual (real fs write) | ↑ |
| **Planner Accuracy** | planner emits the expected capability for a natural-language intent | ↑ |
| **Recovery Success** | when a provider is killed, did a fallback path resolve? | ↑ |
| **Confidence Calibration** | stated confidence ≈ observed success rate (per calibration bin) | ↓ error |

## Chaos Mode (`CHAOS=1`)

Before the run, a subset of providers is taken offline (the browser bridge is
always killed so the `browser → windows` fallback is exercised; Windows, the
fallback target, is kept alive). This surfaces subsystems that have **no fallback
path** — today those are filesystem, terminal, git, and inference, which is the
single biggest reliability gap the harness has found.

## Findings so far

- Normal mode: MTBF ≈ `n` (everything resolves), Simulation/Planner accuracy
  100%, calibration error < 0.08.
- Chaos mode: MTBF collapses to ~3; the `browser → windows` fallback recovers
  100%, but killed filesystem/terminal/git/inference tasks have **no recovery
  path** (no fallback registered for them).
- The declared `browser → windows` fallback was **non-functional**: Windows did
  not advertise `navigate`/`launch`/`wait_ready` capabilities, so the fallback
  could never resolve. (Harness registers mirror capabilities to exercise it.)

## Follow-ups

- Add real fallback providers for filesystem/terminal/git/inference (or a generic
  "retry on alternate provider" path) and re-measure Recovery Success.
- Extend the corpus with Windows-heavy mixes (Explorer, focus, UAC, multi-monitor)
  per the Phase Model risk note.
- Add latency/packet-loss injection to Chaos Mode (beyond provider kills).
- Wire the dashboard JSON into a live reliability dashboard (Grafana/CI artifact).
