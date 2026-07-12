//! `Simulatable` implementation for the Windows bridge (M4.7).
//!
//! Lets the Simulation Engine model `windows.*` intent without a live C# host.
//! The transport is never invoked during simulation, so a null transport is used
//! to construct the adapter for the sim registry.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use friday_capabilities::sim::{
    Precondition, ResourceForecast, RollbackPlan, SideEffect, SimRequest, Simulatable,
    StepEstimate, WorldState,
};

use crate::bridge::WindowsAdapter;
use crate::ipc::IpcTransport;
use crate::protocol::BridgeFrame;

/// Transport that is never called during simulation.
struct NullTransport;

#[async_trait]
impl IpcTransport for NullTransport {
    async fn call(&self, _frame: BridgeFrame) -> Result<BridgeFrame, String> {
        Err("simulation-only transport".into())
    }
}

impl WindowsAdapter {
    /// Construct an adapter suitable for the Simulation Engine (no real host).
    pub fn for_simulation() -> Self {
        WindowsAdapter::new(Arc::new(NullTransport))
    }
}

impl Simulatable for WindowsAdapter {
    fn preconditions(&self, req: &SimRequest, world: &WorldState) -> Vec<Precondition> {
        let service_ok = world
            .service_health
            .get("windows")
            .map(|h| h.available())
            .unwrap_or(true);
        let mut pre = vec![
            Precondition {
                description: "permission 'windows.control' granted".into(),
                kind: "permission".into(),
                satisfied: true,
            },
            Precondition {
                description: "windows service available".into(),
                kind: "service".into(),
                satisfied: service_ok,
            },
        ];
        if req.action == "open" {
            pre.push(Precondition {
                description: "a target app was specified".into(),
                kind: "state".into(),
                satisfied: req
                    .str_param("app")
                    .or_else(|| req.str_param("name"))
                    .is_some(),
            });
        }
        pre
    }

    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect> {
        let app = req.str_param("app").or_else(|| req.str_param("name"));
        match req.action.as_str() {
            "open" => {
                let mut e = SideEffect::leaf("window.launch", "launch window", 1.0)
                    .with_child(SideEffect::leaf("memory.alloc", "memory allocated", 16.0));
                if let Some(a) = app {
                    e = e.with_target(a);
                }
                vec![e]
            }
            "close" => {
                let mut e = SideEffect::leaf("window.close", "close window", 1.0)
                    .with_child(SideEffect::leaf("memory.free", "memory freed", 16.0));
                if let Some(a) = app {
                    e = e.with_target(a);
                }
                vec![e]
            }
            "focus" | "foreground" => {
                vec![SideEffect::leaf("window.focus", "focus window", 0.0)
                    .with_target(app.unwrap_or_default())]
            }
            "move" | "resize" => {
                vec![
                    SideEffect::leaf("window.geometry", "change window geometry", 0.0)
                        .with_target(app.unwrap_or_default()),
                ]
            }
            "capture" => vec![SideEffect::leaf("screen.capture", "capture screen", 0.0)],
            "list" => vec![SideEffect::leaf("window.list", "list windows (read)", 0.0)],
            "click" | "type" | "scroll" => {
                vec![SideEffect::leaf("ui.interaction", "UI interaction", 0.0)]
            }
            "clipboard.set" => vec![SideEffect::leaf("clipboard.set", "set clipboard", 1.0)
                .with_target(req.str_param("content").unwrap_or_default())],
            "clipboard.get" => vec![SideEffect::leaf("clipboard.read", "read clipboard", 0.0)],
            other => vec![SideEffect::leaf(
                format!("windows.{other}"),
                format!("windows action '{other}'"),
                0.0,
            )],
        }
    }

    fn estimate(&self, req: &SimRequest) -> StepEstimate {
        let reliability = match req.action.as_str() {
            "open" => 0.9,
            _ => 0.95,
        };
        StepEstimate {
            latency_ms: 30,
            cost: 1.0,
            reliability,
        }
    }

    fn forecast(&self, req: &SimRequest) -> ResourceForecast {
        match req.action.as_str() {
            "open" => ResourceForecast {
                memory_mb: 16,
                tokens: 20,
                ..Default::default()
            },
            "capture" => ResourceForecast {
                memory_mb: 8,
                tokens: 30,
                ..Default::default()
            },
            _ => ResourceForecast {
                tokens: 10,
                ..Default::default()
            },
        }
    }

    fn rollback(&self, req: &SimRequest) -> Option<RollbackPlan> {
        if req.action == "open" {
            req.str_param("app")
                .or_else(|| req.str_param("name"))
                .map(|a| RollbackPlan {
                    steps: vec![format!("windows.close app={a}")],
                    automatic: false,
                })
        } else {
            None
        }
    }
}

/// Build the `tool -> Simulatable` map for the Windows bridge.
pub fn sim_registry() -> HashMap<String, Arc<dyn Simulatable>> {
    let mut m: HashMap<String, Arc<dyn Simulatable>> = HashMap::new();
    m.insert("windows".into(), Arc::new(WindowsAdapter::for_simulation()));
    m
}
