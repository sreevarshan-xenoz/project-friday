//! `cargo run -p friday-benchmark -- [tasks]`
//!
//! Set `CHAOS=1` in the environment to run a single Chaos Mode run. Add
//! `--save` to run both Normal + Chaos modes and persist a day snapshot to
//! `benchmarks/<date>.json`, then print the historical MTBF trend.

use std::env;
use std::path::PathBuf;

use friday_benchmark::{
    load_history, render_dashboard, render_trend, run_benchmark, run_day, save_day,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    let save = args.iter().any(|a| a == "--save");
    let tasks: usize = args
        .iter()
        .find_map(|s| s.parse::<usize>().ok())
        .unwrap_or(600);

    if save {
        eprintln!("Running FRIDAY benchmark (both modes): {} tasks", tasks);
        let day = run_day(tasks);
        let path = save_day(&day);
        println!("{}", render_dashboard(&day.chaos));
        println!(
            "{}",
            render_trend(&load_history(
                PathBuf::from(friday_benchmark::BENCHMARKS_DIR).as_path()
            ))
        );
        eprintln!(
            "\nNormal MTBF={:.1}  Chaos MTBF={:.1}  sim_acc={:.1}%  planner_acc={:.1}%  cal_err={:.3}",
            day.normal.metrics.mtbf_tasks,
            day.chaos.metrics.mtbf_tasks,
            day.chaos.metrics.simulation_accuracy * 100.0,
            day.chaos.metrics.planner_accuracy * 100.0,
            day.chaos.metrics.calibration_error,
        );
        if let Some(r) = day.chaos.metrics.recovery_success {
            eprintln!("recovery_success={:.1}%", r * 100.0);
        }
        eprintln!("Day snapshot written to {}", path.display());
        return;
    }

    let chaos = env::var("CHAOS").map(|v| v == "1").unwrap_or(false);
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
