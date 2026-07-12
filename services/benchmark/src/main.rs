//! `cargo run -p friday-benchmark -- [tasks]`
//!
//! Set `CHAOS=1` in the environment to run under Chaos Mode (random providers
//! taken offline). Emits a JSON report and a markdown dashboard.

use std::env;
use std::path::PathBuf;

use friday_benchmark::{render_dashboard, run_benchmark};

fn main() {
    let chaos = env::var("CHAOS").map(|v| v == "1").unwrap_or(false);
    let tasks: usize = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);

    eprintln!("Running FRIDAY benchmark: {} tasks, chaos={}", tasks, chaos);

    let result = run_benchmark(chaos, tasks);

    let out_dir = PathBuf::from("target/friday-bench");
    let _ = std::fs::create_dir_all(&out_dir);
    let json = serde_json::to_string_pretty(&result).expect("serialize");
    std::fs::write(out_dir.join("report.json"), &json).expect("write json");
    std::fs::write(out_dir.join("dashboard.md"), render_dashboard(&result)).expect("write md");

    print!("{}", render_dashboard(&result));
    eprintln!(
        "\nMTBF={:.1}  sim_acc={:.1}%  planner_acc={:.1}%  cal_err={:.3}",
        result.metrics.mtbf_tasks,
        result.metrics.simulation_accuracy * 100.0,
        result.metrics.planner_accuracy * 100.0,
        result.metrics.calibration_error,
    );
    if let Some(r) = result.metrics.recovery_success {
        eprintln!("recovery_success={:.1}%", r * 100.0);
    }
    eprintln!("Reports written to {}", out_dir.display());
}
