//! `Simulatable` implementations for the built-in service adapters (M4.7).
//!
//! Each adapter declares, per action, what must be true (`preconditions`), what
//! would happen (`side_effects`), how expensive it is (`estimate`/`forecast`), and
//! how it could be undone (`rollback`). This is what lets the Simulation Engine
//! build a Digital Twin of the agent without ever touching the real system.

use std::collections::HashMap;
use std::sync::Arc;

use friday_capabilities::sim::{
    Precondition, ResourceForecast, RollbackPlan, SideEffect, SimRequest, Simulatable,
    StepEstimate, WorldState,
};

use crate::adapters::{FilesystemAdapter, GitAdapter, TerminalAdapter};

fn content_size(req: &SimRequest) -> i64 {
    req.str_param("content")
        .map(|c| c.len() as i64)
        .unwrap_or(0)
}

// ── Filesystem ────────────────────────────────────────────────────────────

impl Simulatable for FilesystemAdapter {
    fn preconditions(&self, req: &SimRequest, world: &WorldState) -> Vec<Precondition> {
        let permission = match req.action.as_str() {
            "read_file" | "list_directory" => "fs.read",
            _ => "fs.write",
        };
        vec![
            Precondition {
                description: "filesystem is writable".into(),
                kind: "state".into(),
                satisfied: world.filesystem.writable,
            },
            Precondition {
                description: format!("permission '{permission}' granted"),
                kind: "permission".into(),
                satisfied: true,
            },
        ]
    }

    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect> {
        let size = content_size(req);
        let path = req.str_param("path");
        match req.action.as_str() {
            "create_file" | "write_file" => {
                let target = path.clone();
                let mut e = SideEffect::leaf("file.create", "create/overwrite file", 1.0)
                    .with_child(SideEffect::leaf(
                        "disk.usage",
                        "disk space used",
                        size as f64,
                    ))
                    .with_child(SideEffect::leaf(
                        "filesystem.change",
                        "filesystem mutated",
                        1.0,
                    ));
                if let Some(t) = target {
                    e = e.with_target(t);
                }
                vec![e]
            }
            "delete_file" => {
                let target = path.clone();
                let mut e = SideEffect::leaf("file.delete", "delete file", 1.0)
                    .with_child(SideEffect::leaf(
                        "disk.usage",
                        "disk space freed",
                        -(size as f64),
                    ))
                    .with_child(SideEffect::leaf(
                        "filesystem.change",
                        "filesystem mutated",
                        1.0,
                    ));
                if let Some(t) = target {
                    e = e.with_target(t);
                }
                vec![e]
            }
            "create_dir" | "mkdir" => vec![SideEffect::leaf("dir.create", "create directory", 1.0)
                .with_target(path.unwrap_or_default())],
            "delete_dir" | "rmdir" => vec![SideEffect::leaf("dir.delete", "delete directory", 1.0)
                .with_target(path.unwrap_or_default())],
            "move" | "rename" => vec![SideEffect::leaf("file.move", "move/rename file", 1.0)
                .with_target(req.str_param("to").unwrap_or_default())],
            "copy" => vec![SideEffect::leaf("file.copy", "copy file", 1.0)
                .with_target(req.str_param("to").unwrap_or_default())],
            "read_file" => vec![
                SideEffect::leaf("file.read", "read file (no mutation)", 0.0)
                    .with_target(path.unwrap_or_default()),
            ],
            "list_directory" => {
                vec![
                    SideEffect::leaf("dir.list", "list directory (no mutation)", 0.0)
                        .with_target(path.unwrap_or_default()),
                ]
            }
            other => vec![SideEffect::leaf(
                format!("filesystem.{other}"),
                format!("filesystem action '{other}'"),
                0.0,
            )],
        }
    }

    fn estimate(&self, req: &SimRequest) -> StepEstimate {
        let size = content_size(req) as f64;
        StepEstimate {
            latency_ms: 5,
            cost: (size / 1024.0).clamp(0.0, 50.0),
            reliability: 0.999,
        }
    }

    fn forecast(&self, req: &SimRequest) -> ResourceForecast {
        let size = content_size(req);
        ResourceForecast {
            cpu_pct: 0.5,
            memory_mb: 1,
            disk_bytes: size,
            tokens: match req.action.as_str() {
                "read_file" => 200,
                _ => 0,
            },
            ..Default::default()
        }
    }

    fn rollback(&self, req: &SimRequest) -> Option<RollbackPlan> {
        match req.action.as_str() {
            "create_file" | "write_file" => req.str_param("path").map(|p| RollbackPlan {
                steps: vec![format!("delete {p}")],
                automatic: false,
            }),
            "delete_file" => Some(RollbackPlan {
                steps: vec!["restore from recycle bin".into()],
                automatic: false,
            }),
            "create_dir" | "mkdir" => req.str_param("path").map(|p| RollbackPlan {
                steps: vec![format!("rmdir {p}")],
                automatic: false,
            }),
            _ => None,
        }
    }
}

// ── Terminal ──────────────────────────────────────────────────────────────

impl Simulatable for TerminalAdapter {
    fn preconditions(&self, _req: &SimRequest, _world: &WorldState) -> Vec<Precondition> {
        vec![Precondition {
            description: "permission 'terminal.execute' granted".into(),
            kind: "permission".into(),
            satisfied: true,
        }]
    }

    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect> {
        let cmd = req.str_param("command").unwrap_or_default();
        vec![
            SideEffect::leaf("process.spawn", "spawn shell process", 1.0)
                .with_child(
                    SideEffect::leaf("shell.command", "shell command executed", 1.0)
                        .with_target(cmd),
                )
                .with_child(SideEffect::leaf("cpu.usage", "cpu time used", 2.0)),
        ]
    }

    fn estimate(&self, _req: &SimRequest) -> StepEstimate {
        StepEstimate {
            latency_ms: 50,
            cost: 1.0,
            reliability: 0.99,
        }
    }

    fn forecast(&self, _req: &SimRequest) -> ResourceForecast {
        ResourceForecast {
            cpu_pct: 5.0,
            memory_mb: 32,
            tokens: 50,
            ..Default::default()
        }
    }

    fn rollback(&self, _req: &SimRequest) -> Option<RollbackPlan> {
        None
    }
}

// ── Git ───────────────────────────────────────────────────────────────────

impl Simulatable for GitAdapter {
    fn preconditions(&self, req: &SimRequest, _world: &WorldState) -> Vec<Precondition> {
        let permission = match req.action.as_str() {
            "status" | "diff" | "log" | "branch" => "git.read",
            _ => "git.write",
        };
        vec![Precondition {
            description: format!("permission '{permission}' granted"),
            kind: "permission".into(),
            satisfied: true,
        }]
    }

    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect> {
        match req.action.as_str() {
            "clone" => vec![SideEffect::leaf("git.clone", "clone repository", 1.0)
                .with_child(SideEffect::leaf(
                    "disk.usage",
                    "disk space used",
                    1_000_000.0,
                ))
                .with_child(SideEffect::leaf(
                    "network.usage",
                    "network used",
                    5_000_000.0,
                ))],
            "commit" => vec![SideEffect::leaf("git.commit", "create commit", 1.0)
                .with_child(SideEffect::leaf("repo.change", "repository mutated", 1.0))
                .with_child(SideEffect::leaf("disk.usage", "disk space used", 10.0))],
            "push" | "pull" => {
                vec![
                    SideEffect::leaf(format!("git.{}", req.action), req.action.clone(), 1.0)
                        .with_child(SideEffect::leaf(
                            "network.usage",
                            "network used",
                            2_000_000.0,
                        )),
                ]
            }
            "merge" => vec![SideEffect::leaf("git.merge", "merge branches", 1.0)
                .with_child(SideEffect::leaf("repo.change", "repository mutated", 1.0))],
            "init" => vec![SideEffect::leaf("git.init", "initialize repository", 1.0)],
            _ => vec![SideEffect::leaf(
                format!("git.{}", req.action),
                req.action.clone(),
                0.0,
            )],
        }
    }

    fn estimate(&self, req: &SimRequest) -> StepEstimate {
        let (latency, reliability) = match req.action.as_str() {
            "clone" => (2000, 0.95),
            "push" | "pull" => (1500, 0.96),
            "commit" | "merge" => (100, 0.98),
            _ => (50, 0.999),
        };
        StepEstimate {
            latency_ms: latency,
            cost: 1.0,
            reliability,
        }
    }

    fn forecast(&self, req: &SimRequest) -> ResourceForecast {
        match req.action.as_str() {
            "clone" => ResourceForecast {
                network_bytes: 5_000_000,
                disk_bytes: 1_000_000,
                tokens: 100,
                ..Default::default()
            },
            "push" | "pull" => ResourceForecast {
                network_bytes: 2_000_000,
                tokens: 100,
                ..Default::default()
            },
            _ => ResourceForecast {
                memory_mb: 4,
                disk_bytes: 10,
                tokens: 100,
                ..Default::default()
            },
        }
    }

    fn rollback(&self, req: &SimRequest) -> Option<RollbackPlan> {
        match req.action.as_str() {
            "commit" => Some(RollbackPlan {
                steps: vec!["git reset --soft HEAD~1".into()],
                automatic: false,
            }),
            "clone" => req.str_param("repo").map(|r| RollbackPlan {
                steps: vec![format!("remove {r}")],
                automatic: false,
            }),
            "merge" => Some(RollbackPlan {
                steps: vec!["git merge --abort".into()],
                automatic: false,
            }),
            _ => None,
        }
    }
}

/// Build the `tool -> Simulatable` map for the built-in services.
pub fn sim_registry() -> HashMap<String, Arc<dyn Simulatable>> {
    let mut m: HashMap<String, Arc<dyn Simulatable>> = HashMap::new();
    m.insert("filesystem".into(), Arc::new(FilesystemAdapter::new()));
    m.insert("terminal".into(), Arc::new(TerminalAdapter::new()));
    m.insert("git".into(), Arc::new(GitAdapter::new()));
    m
}
