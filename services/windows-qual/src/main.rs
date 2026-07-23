//! `windows-qual` — Sprint 001A Windows Qualification Harness.
//!
//! Usage:
//!   windows-qual                 # real mode: talks to the running C# windows-host
//!   windows-qual --mock          # in-process mock, no .NET required
//!   windows-qual --session S     # pipe session (default "default")
//!   windows-qual --out DIR       # evidence pack directory
//!   windows-qual --scenario ID   # run only the scenario with this id
//!   windows-qual --replay PATH   # re-run the scenario recorded in PATH
//!                                #   (report.json or trace.json of an evidence pack)
//!
//! Pipeline: IPC -> execute -> measure -> verify (State Accuracy) -> report
//! (Evidence Packs + benchmark-linked issues). No recovery, no Desktop State
//! Model yet — just facts.
//!
//! Mock mode is unit testing. Real mode is qualification.

mod candidates;
mod corpus;
mod environment;
mod evidence;
mod harness;
mod issue;
mod mock;

use std::path::PathBuf;

use friday_windows::WindowsAdapter;

use candidates::{CandidateStore, GRADUATE_THRESHOLD};
use corpus::all;
use evidence::{aggregate, render, write_candidate, write_environment, write_issue, write_outcome};
use harness::{run_scenario, Mode};

fn build_adapter(mock: bool, session: &str) -> WindowsAdapter {
    if mock {
        return mock::mock_adapter();
    }
    #[cfg(windows)]
    {
        WindowsAdapter::for_session(session)
    }
    #[cfg(not(windows))]
    {
        let _ = session;
        mock::mock_adapter()
    }
}

/// Select scenarios from the corpus by `--scenario` / `--replay` filters.
fn select_scenarios(
    scenario_filter: &Option<String>,
    replay: &Option<String>,
) -> Vec<corpus::Scenario> {
    let mut scen = all();
    if let Some(id) = scenario_filter {
        scen.retain(|s| s.id == id.as_str());
    }
    if let Some(path) = replay {
        let id = read_replay_id(path);
        scen.retain(|s| s.id == id.as_str());
    }
    scen
}

/// Read the scenario `id` recorded in a report.json / trace.json evidence file
/// (or a directory containing one).
fn read_replay_id(path: &str) -> String {
    let p = PathBuf::from(path);
    let f = if p.is_dir() {
        p.join("report.json")
    } else {
        p.clone()
    };
    std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut mock = false;
    let mut session = "default".to_string();
    let mut out: Option<String> = None;
    let mut scenario_filter: Option<String> = None;
    let mut replay: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mock" => mock = true,
            "--session" => {
                if let Some(v) = args.get(i + 1) {
                    session = v.clone();
                }
                i += 1;
            }
            "--out" => {
                if let Some(v) = args.get(i + 1) {
                    out = Some(v.clone());
                }
                i += 1;
            }
            "--scenario" => {
                if let Some(v) = args.get(i + 1) {
                    scenario_filter = Some(v.clone());
                }
                i += 1;
            }
            "--replay" => {
                if let Some(v) = args.get(i + 1) {
                    replay = Some(v.clone());
                }
                i += 1;
            }
            other => eprintln!("ignoring unknown arg: {other}"),
        }
        i += 1;
    }

    let stamp = chrono::Utc::now().format("%Y-%m-%dt%H%M%S").to_string();
    let out_dir = PathBuf::from(out.unwrap_or_else(|| format!("benchmarks/windows-qual/{stamp}")));
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let mode = if mock { Mode::Mock } else { Mode::Real };
    let adapter = build_adapter(mock, &session);

    let env = environment::capture(mode);
    write_environment(&out_dir, &env).expect("write environment");

    println!(
        "Windows Qualification Harness (Sprint 001A) — mode={} session={}\n",
        if mock { "mock" } else { "real" },
        session
    );

    let scenarios = select_scenarios(&scenario_filter, &replay);
    if scenarios.is_empty() {
        eprintln!("no scenarios matched the filter");
        return;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let cand_path = PathBuf::from("benchmarks/windows-qual/candidates.json");
    let mut store = CandidateStore::load(&cand_path);

    let mut outcomes = Vec::new();
    let mut real_issues: Vec<(issue::Issue, usize)> = Vec::new();
    let mut candidate_issues: Vec<(issue::Issue, usize)> = Vec::new();

    for s in &scenarios {
        let o = run_scenario(&adapter, s, mode).await;
        for iss in &o.issues {
            let (count, graduated) = store.observe(iss, &now);
            if graduated {
                let mut real = iss.clone();
                real.status = "open".into();
                write_issue(&out_dir, &real).expect("write issue");
                real_issues.push((real, count));
            } else {
                let mut cand = iss.clone();
                cand.status = format!("candidate (seen {count} time(s)) — pending human review");
                write_candidate(&out_dir, &cand).expect("write candidate");
                candidate_issues.push((cand, count));
            }
        }
        write_outcome(&out_dir, &o).expect("write evidence pack");
        println!(
            "  [{}] {:<28} ok={} state_acc={:?} {}",
            if o.passed { "PASS" } else { "FAIL" },
            o.id,
            o.action_ok,
            o.state_accuracy,
            o.note
        );
        outcomes.push(o);
    }

    store.save(&cand_path).expect("save candidate store");

    let metrics = aggregate(&outcomes);
    let summary = render(&metrics);
    println!("\n{}", summary);

    if !real_issues.is_empty() {
        println!("## Benchmark-linked issues filed (graduated)\n");
        for (iss, count) in &real_issues {
            println!("  {}  {}  (seen {} times)", iss.id, iss.title, count);
        }
        println!("\nSee {}/issues/", out_dir.display());
    }
    if !candidate_issues.is_empty() {
        println!("## Candidate issues (pending human review)\n");
        for (iss, count) in &candidate_issues {
            println!(
                "  {}  {}  (seen {} time(s); graduates after {})",
                iss.id, iss.title, count, GRADUATE_THRESHOLD
            );
        }
        println!(
            "\nSee {}/candidates/  and  benchmarks/windows-qual/candidates.json",
            out_dir.display()
        );
    }

    println!("\nEvidence packs: {}", out_dir.display());
}
