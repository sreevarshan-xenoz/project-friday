//! Default recovery policies — the project's baseline Recovery Graph.
//!
//! Authored per the Reliability Sprint examples. Recovery is action-level: each
//! capability has a tailored plan per [`FailureCategory`]. Policies are data;
//! they can be overridden by loading JSON over this graph.

use crate::policy::{RecoveryGraph, RecoveryPolicy};
use crate::taxonomy::{
    FailureCategory::{Capability, Execution, Permission, Provider, Resource, Timeout},
    RecoveryAction,
};

pub fn default_graph() -> RecoveryGraph {
    let mut g = RecoveryGraph::new();

    // Browser.Navigate -> Windows.OpenURL / Browser.Restart / AskUser
    g.register(
        "browser.navigate",
        RecoveryPolicy::default()
            .with(
                Provider,
                vec![
                    RecoveryAction::UseAlternateCapability {
                        capability: "windows.navigate".into(),
                    },
                    RecoveryAction::RestartService {
                        provider: "browser".into(),
                    },
                    RecoveryAction::AskUser,
                ],
            )
            .with(
                Timeout,
                vec![
                    RecoveryAction::Retry { attempts: 2 },
                    RecoveryAction::AskUser,
                ],
            )
            .with(
                Capability,
                vec![
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::RestartService {
                        provider: "browser".into(),
                    },
                    RecoveryAction::AskUser,
                ],
            )
            .with(
                Execution,
                vec![
                    RecoveryAction::RestartService {
                        provider: "browser".into(),
                    },
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::AskUser,
                ],
            ),
    );

    // Filesystem.Write -> PermissionDenied -> SuggestElevation -> AskUser
    for cap in [
        "filesystem.create_file",
        "filesystem.write_file",
        "filesystem.delete_file",
    ] {
        g.register(
            cap,
            RecoveryPolicy::default()
                .with(
                    Provider,
                    vec![
                        RecoveryAction::Retry { attempts: 2 },
                        RecoveryAction::AskUser,
                    ],
                )
                .with(
                    Permission,
                    vec![RecoveryAction::SuggestElevation, RecoveryAction::AskUser],
                )
                .with(
                    Resource,
                    vec![
                        RecoveryAction::Retry { attempts: 1 },
                        RecoveryAction::AskUser,
                    ],
                )
                .with(
                    Timeout,
                    vec![
                        RecoveryAction::Retry { attempts: 2 },
                        RecoveryAction::AskUser,
                    ],
                ),
        );
    }

    // Terminal -> RestartShell -> RestoreSession -> Retry
    g.register(
        "terminal.run",
        RecoveryPolicy::default()
            .with(
                Provider,
                vec![
                    RecoveryAction::RestartService {
                        provider: "terminal".into(),
                    },
                    RecoveryAction::RestoreSession,
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::AskUser,
                ],
            )
            .with(
                Timeout,
                vec![
                    RecoveryAction::RestartService {
                        provider: "terminal".into(),
                    },
                    RecoveryAction::RestoreSession,
                    RecoveryAction::Retry { attempts: 2 },
                    RecoveryAction::AskUser,
                ],
            ),
    );

    // Git -> Wait -> Cleanup -> Retry (different strategy, not another provider)
    for cap in ["git.add", "git.commit", "git.push", "git.pull", "git.merge"] {
        g.register(
            cap,
            RecoveryPolicy::default()
                .with(
                    Provider,
                    vec![
                        RecoveryAction::WaitAndRetry { ms: 500 },
                        RecoveryAction::CleanupAndRetry {
                            cleanup: "stale lock".into(),
                        },
                        RecoveryAction::Retry { attempts: 1 },
                        RecoveryAction::AskUser,
                    ],
                )
                .with(
                    Resource,
                    vec![
                        RecoveryAction::WaitAndRetry { ms: 1000 },
                        RecoveryAction::Retry { attempts: 1 },
                        RecoveryAction::AskUser,
                    ],
                ),
        );
    }

    // Inference -> Retry same -> Retry different provider -> Escalate
    g.register(
        "inference.complete",
        RecoveryPolicy::default()
            .with(
                Provider,
                vec![
                    RecoveryAction::Retry { attempts: 2 },
                    RecoveryAction::Escalate,
                ],
            )
            .with(
                Capability,
                vec![
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::UseProvider {
                        capability: "inference.complete".into(),
                    },
                    RecoveryAction::Escalate,
                ],
            )
            .with(
                Timeout,
                vec![
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::Escalate,
                ],
            ),
    );

    // Windows -> Retry -> AskUser
    g.register(
        "windows.open",
        RecoveryPolicy::default()
            .with(
                Provider,
                vec![
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::AskUser,
                ],
            )
            .with(
                Timeout,
                vec![
                    RecoveryAction::Retry { attempts: 1 },
                    RecoveryAction::AskUser,
                ],
            ),
    );

    g
}

/// Number of actions that have at least one registered policy.
pub fn policy_count(g: &RecoveryGraph) -> usize {
    g.actions().len()
}

/// Build a graph from a JSON map of `action -> { category -> [steps] }`.
pub fn from_json(json: &str) -> Result<RecoveryGraph, String> {
    let g: RecoveryGraph =
        serde_json::from_str(json).map_err(|e| format!("invalid recovery graph: {e}"))?;
    Ok(g)
}

/// Serialize a graph back out (for persistence / dashboards).
pub fn to_json(g: &RecoveryGraph) -> String {
    serde_json::to_string_pretty(g).expect("serialize recovery graph")
}
