//! Confidence-calibration and reliability smoke tests for the benchmark harness.

use friday_benchmark::{measure_planner_accuracy, run_benchmark};

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
    assert!(r.metrics.mtbf_tasks.is_finite() && r.metrics.mtbf_tasks > 0.0);
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
    assert!(r.metrics.recovery_success.is_some());
}

#[test]
fn planner_accuracy_is_deterministic() {
    let a = measure_planner_accuracy();
    let b = measure_planner_accuracy();
    assert_eq!(a, b);
    assert_eq!(a, 1.0);
}
