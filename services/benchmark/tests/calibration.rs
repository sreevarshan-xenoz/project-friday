//! Confidence-calibration and reliability smoke tests for the benchmark harness.

use friday_benchmark::{measure_planner_accuracy, run_benchmark, run_production_pilot};

#[test]
fn benchmark_runs_and_produces_metrics() {
    let r = run_benchmark(false, 300);
    assert!(r.metrics.total_tasks > 0, "no tasks ran");
    assert!(!r.calibration.is_empty(), "no calibration bins");
    // With no chaos and a healthy registry, the Digital Twin should match the
    // sandbox for filesystem tasks and resolve the rest.
    assert!(
        r.metrics.simulation_accuracy >= 0.8,
        "simulation accuracy too low: {:.2}",
        r.metrics.simulation_accuracy
    );
    assert!(
        r.metrics.planner_accuracy >= 0.99,
        "planner accuracy dropped: {:.2}",
        r.metrics.planner_accuracy
    );
    assert!(r.metrics.raw_mtbf.is_finite() && r.metrics.raw_mtbf > 0.0);
    // Calibration error should be small when the model is honest.
    assert!(
        r.metrics.calibration_error <= 0.25,
        "calibration error too high: {:.3}",
        r.metrics.calibration_error
    );
}

#[test]
fn chaos_mode_runs_and_measures_recovery() {
    let r = run_benchmark(true, 300);
    assert!(r.metrics.total_tasks > 0);
    // Under chaos some providers are offline; the run must still complete and
    // surface a recovery figure (browser -> windows fallback).
    assert!(r.metrics.recovery_effectiveness.is_some());
}

#[test]
fn planner_accuracy_is_deterministic() {
    let a = measure_planner_accuracy();
    let b = measure_planner_accuracy();
    assert_eq!(a, b);
    assert_eq!(a, 1.0);
}

#[test]
fn production_pilot_verifies_healthy_creates_with_independent_ground_truth() {
    let n = 40usize;
    let p = run_production_pilot(n);
    // Every healthy (P0) create must succeed at the kernel AND independently
    // verify against the filesystem — no drift between the two.
    assert_eq!(p.metrics.p0_healthy, n, "all P0 creates attempted");
    assert_eq!(
        p.metrics.p0_verified, n,
        "all P0 creates independently verified"
    );
    assert_eq!(
        p.metrics.attempted,
        n + 2,
        "n P0 + p1 + p2 operations (p3 is a verdict only)"
    );
    assert_eq!(
        p.metrics.mismatches, 0,
        "kernel/ground-truth must never disagree"
    );
    assert_eq!(p.metrics.verified_success, n, "only the P0 creates verify");
    // The production track is a separate block — it must not surface in Metrics.
    assert!(p.mode.contains("Production"));

    let p0: Vec<_> = p.tasks.iter().filter(|t| t.scenario == "p0").collect();
    assert_eq!(p0.len(), n);
    assert!(p0.iter().all(|t| t.verified_success && !t.mismatch));
}

#[test]
fn production_pilot_classifies_failures_explicitly() {
    let p = run_production_pilot(10);
    // P1 (invalid target) must be an explicit execution failure, never unknown.
    let p1 = p.tasks.iter().find(|t| t.scenario == "p1").unwrap();
    assert!(!p1.kernel_success && !p1.ground_truth);
    assert!(p1.classification != "unknown" && p1.classification != "success");

    // P2 (kernel permission gate) must produce an explicit permission denial.
    let p2 = p.tasks.iter().find(|t| t.scenario == "p2").unwrap();
    assert!(!p2.kernel_success && !p2.ground_truth);
    assert_eq!(p2.classification, "permission");

    // P3 must consult the Recovery Graph and record an explicit verdict —
    // never a success and never swallowed into 'unknown'.
    let p3 = p.tasks.iter().find(|t| t.scenario == "p3").unwrap();
    assert!(p3.recovery_chosen.is_some());
    assert!(p3.recovery_recovered.is_some());
    assert_eq!(p3.classification, "permission");
}

#[test]
fn production_pilot_is_deterministic() {
    let a = run_production_pilot(25);
    let b = run_production_pilot(25);
    assert_eq!(a.metrics.attempted, b.metrics.attempted);
    assert_eq!(a.metrics.verified_success, b.metrics.verified_success);
    assert_eq!(a.metrics.mismatches, b.metrics.mismatches);
    assert_eq!(a.metrics.p0_verified, b.metrics.p0_verified);
}
