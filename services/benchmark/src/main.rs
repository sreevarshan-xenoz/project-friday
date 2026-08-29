//! `cargo run -p friday-benchmark -- [tasks]`
//!
//! Set `CHAOS=1` in the environment to run a single Chaos Mode run. Add
//! `--save` to run both Normal + Chaos modes and persist a day snapshot to
//! `benchmarks/<date>.json`, then print the historical MTBF trend.

use std::env;
use std::path::PathBuf;

use friday_benchmark::{
    load_history, render_dashboard, render_production, render_trend, run_benchmark, run_day,
    run_production_pilot, save_day,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    let save = args.iter().any(|a| a == "--save");
    let production = args.iter().any(|a| a == "--production");
    let tasks: usize = args
        .iter()
        .find_map(|s| s.parse::<usize>().ok())
        .unwrap_or(600);

    if production {
        eprintln!(
            "Running FRIDAY Production Execution Pilot: {} operations",
            tasks
        );
        let pilot = run_production_pilot(tasks);
        let out_dir = PathBuf::from("target/friday-bench");
        let _ = std::fs::create_dir_all(&out_dir);
        let json = serde_json::to_string_pretty(&pilot).expect("serialize pilot");
        std::fs::write(out_dir.join("production.json"), &json).expect("write pilot json");
        let md = render_production(&pilot);
        std::fs::write(out_dir.join("production.md"), &md).expect("write pilot md");
        print!("{}", md);
        eprintln!(
            "Pilot: attempted={} verified={} ({:.1}%) mismatches={}",
            pilot.metrics.attempted,
            pilot.metrics.verified_success,
            if pilot.metrics.attempted > 0 {
                pilot.metrics.verified_success as f64 / pilot.metrics.attempted as f64 * 100.0
            } else {
                0.0
            },
            pilot.metrics.mismatches
        );
        eprintln!("Pilot reports written to {}", out_dir.display());
        return;
    }

    if save {
        eprintln!("Running FRIDAY benchmark (both modes): {} tasks", tasks);
        let history = load_history(PathBuf::from(friday_benchmark::BENCHMARKS_DIR).as_path());
        let prior = history
            .last()
            .map(|d| d.confidence_model.clone())
            .unwrap_or_default();
        let day = run_day(tasks, &prior);
        let path = save_day(&day);
        println!("{}", render_dashboard(&day.chaos));
        println!(
            "{}",
            render_trend(&load_history(
                PathBuf::from(friday_benchmark::BENCHMARKS_DIR).as_path()
            ))
        );
        eprintln!(
            "\nNormal Raw MTBF={:.1}  Chaos Raw MTBF={:.1}  Chaos User Success={:.1}%  Recovery Eff={:?}  sim_acc={:.1}%  planner_acc={:.1}%  cal_err={:.3}",
            day.normal.metrics.raw_mtbf,
            day.chaos.metrics.raw_mtbf,
            day.chaos.metrics.user_success_rate * 100.0,
            day.chaos.metrics.recovery_effectiveness,
            day.chaos.metrics.simulation_accuracy * 100.0,
            day.chaos.metrics.planner_accuracy * 100.0,
            day.chaos.metrics.calibration_error,
        );
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
        "\nRaw MTBF={:.1}  User Success={:.1}%  First-Pass={:.1}%  sim_acc={:.1}%  planner_acc={:.1}%  cal_err={:.3}",
        result.metrics.raw_mtbf,
        result.metrics.user_success_rate * 100.0,
        result.metrics.first_pass_rate * 100.0,
        result.metrics.simulation_accuracy * 100.0,
        result.metrics.planner_accuracy * 100.0,
        result.metrics.calibration_error,
    );
    if let Some(r) = result.metrics.recovery_effectiveness {
        eprintln!("recovery_effectiveness={:.1}%", r * 100.0);
    }
    eprintln!("Reports written to {}", out_dir.display());
}
