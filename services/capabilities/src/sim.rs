//! World Model + Plan Simulation primitives (M4.7).
//!
//! This module defines the contract every service implements so the Simulation
//! Engine can virtualize an entire execution against an **immutable snapshot of
//! the world** ([`WorldState`]) instead of touching live objects. The
//! [`Simulatable`] trait is richer than "dry run": each service declares what must
//! already be true ([`Precondition`]), what would happen ([`SideEffect`]), how
//! expensive it is ([`StepEstimate`]/[`ResourceForecast`]), and how it could be
//! undone ([`RollbackPlan`]).
//!
//! It lives in `friday-capabilities` (not in `friday-simulation`) so adapters in
//! `friday-actions` / `friday-windows` can implement it without a dependency cycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::model::{Capability, Health};
use crate::registry::CapabilityRegistry;

/// A virtual action handed to a [`Simulatable`]. Mirrors `ActionRequest` minus the
/// id/timeout/permission-hint fields the kernel manages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimRequest {
    pub tool: String,
    pub action: String,
    pub params: serde_json::Value,
}

impl SimRequest {
    pub fn new(
        tool: impl Into<String>,
        action: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            tool: tool.into(),
            action: action.into(),
            params,
        }
    }

    /// Extract a required string parameter, if present.
    pub fn str_param(&self, key: &str) -> Option<String> {
        self.params
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

/// A structured side effect of a simulated action. Forms a tree via `children`,
/// which is how the engine builds the **side-effect graph** and chains predictions
/// (e.g. `Create File → +file → +disk_usage → +filesystem_change`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideEffect {
    /// Effect kind, namespaced like `file.delete`, `memory.alloc`, `git.commit`.
    pub kind: String,
    pub description: String,
    /// Optional target of the effect (a path, url, app name, …).
    pub target: Option<String>,
    /// Numeric magnitude of the effect (e.g. +1 file, +250 MB). May be negative.
    pub magnitude: f64,
    pub children: Vec<SideEffect>,
}

impl SideEffect {
    pub fn leaf(kind: impl Into<String>, description: impl Into<String>, magnitude: f64) -> Self {
        Self {
            kind: kind.into(),
            description: description.into(),
            target: None,
            magnitude,
            children: Vec::new(),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_child(mut self, child: SideEffect) -> Self {
        self.children.push(child);
        self
    }

    /// Flatten the tree into a list (depth-first).
    pub fn flatten(&self) -> Vec<&SideEffect> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.flatten());
        }
        out
    }
}

/// Forecast of resource usage contributed by one or many actions.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ResourceForecast {
    pub cpu_pct: f64,
    pub memory_mb: u64,
    pub network_bytes: u64,
    pub disk_bytes: i64,
    /// GPU utilization percent.
    pub gpu_pct: f64,
    pub tokens: u64,
}

impl ResourceForecast {
    /// Sum two forecasts (used when accumulating sequential steps).
    pub fn combine(&self, other: &ResourceForecast) -> ResourceForecast {
        ResourceForecast {
            cpu_pct: self.cpu_pct + other.cpu_pct,
            memory_mb: self.memory_mb + other.memory_mb,
            network_bytes: self.network_bytes + other.network_bytes,
            disk_bytes: self.disk_bytes + other.disk_bytes,
            gpu_pct: self.gpu_pct + other.gpu_pct,
            tokens: self.tokens + other.tokens,
        }
    }

    /// Take the peak of two forecasts (used for peak memory / peak cpu across a plan).
    pub fn peak_with(&self, other: &ResourceForecast) -> ResourceForecast {
        ResourceForecast {
            cpu_pct: self.cpu_pct.max(other.cpu_pct),
            memory_mb: self.memory_mb.max(other.memory_mb),
            network_bytes: self.network_bytes.max(other.network_bytes),
            disk_bytes: self.disk_bytes.max(other.disk_bytes),
            gpu_pct: self.gpu_pct.max(other.gpu_pct),
            tokens: self.tokens.max(other.tokens),
        }
    }
}

/// A precondition that must hold for an action to be valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precondition {
    pub description: String,
    pub kind: String,
    pub satisfied: bool,
}

/// How an action could be undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPlan {
    pub steps: Vec<String>,
    pub automatic: bool,
}

/// Per-step cost/quality estimate.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct StepEstimate {
    pub latency_ms: u64,
    pub cost: f64,
    pub reliability: f64,
}

/// Confidence/prediction attached to a step or the whole plan. Deterministic:
/// no ML. `assumptions` make the number auditable and improvable at M9/M10.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub confidence: f64,
    pub uncertainty: f64,
    pub assumptions: Vec<String>,
}

/// Result of simulating a single action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimResult {
    pub effects: Vec<SideEffect>,
    pub forecast: ResourceForecast,
    pub estimate: StepEstimate,
    pub preconditions: Vec<Precondition>,
    pub rollback: Option<RollbackPlan>,
}

/// A window known to the world model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub app: String,
}

/// Filesystem summary used by the Plan Diff.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesystemState {
    /// Count of items per top-level path (used for "Downloads: 143 files").
    pub path_counts: HashMap<String, u64>,
    pub writable: bool,
}

/// Immutable snapshot of the world the simulator runs against. Never mutated in
/// place; [`WorldState::apply_effects`] returns a new snapshot for the Plan Diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub capabilities: Vec<Capability>,
    pub service_health: HashMap<String, Health>,
    pub windows: Vec<WindowInfo>,
    pub sessions: Vec<String>,
    pub resources: ResourceForecast,
    pub filesystem: FilesystemState,
    pub clipboard: Option<String>,
    pub browser_tabs: Vec<String>,
    pub terminal_state: String,
    pub active_user: String,
    pub time: DateTime<Utc>,
}

impl WorldState {
    /// Build the capability/health snapshot from the registry. Callers set the
    /// live fields (windows, browser, filesystem, …) from real probes.
    pub fn from_registry(registry: &CapabilityRegistry) -> Self {
        let all = registry.all();
        let mut capabilities = Vec::new();
        let mut service_health: HashMap<String, Health> = HashMap::new();
        for (provider, cap) in &all {
            if !service_health.contains_key(provider) {
                service_health.insert(provider.clone(), cap.health);
            }
            capabilities.push(cap.clone());
        }
        WorldState {
            capabilities,
            service_health,
            windows: Vec::new(),
            sessions: Vec::new(),
            resources: ResourceForecast::default(),
            filesystem: FilesystemState {
                path_counts: HashMap::new(),
                writable: true,
            },
            clipboard: None,
            browser_tabs: Vec::new(),
            terminal_state: String::new(),
            active_user: "user".to_string(),
            time: Utc::now(),
        }
    }

    pub fn with_windows(mut self, windows: Vec<WindowInfo>) -> Self {
        self.windows = windows;
        self
    }

    pub fn with_browser_tabs(mut self, tabs: Vec<String>) -> Self {
        self.browser_tabs = tabs;
        self
    }

    pub fn with_filesystem(mut self, fs: FilesystemState) -> Self {
        self.filesystem = fs;
        self
    }

    pub fn with_clipboard(mut self, clip: Option<String>) -> Self {
        self.clipboard = clip;
        self
    }

    /// Derive the *after* world state by applying simulated effects. Used to show
    /// the user the **change**, not the step list (Plan Diff).
    pub fn apply_effects(&self, effects: &[SideEffect]) -> WorldState {
        let mut next = self.clone();
        for effect in effects {
            for e in effect.flatten() {
                match e.kind.as_str() {
                    "file.create" | "dir.create" => {
                        if let Some(t) = &e.target {
                            *next.filesystem.path_counts.entry(t.clone()).or_insert(0) += 1;
                        }
                    }
                    "file.delete" | "dir.delete" => {
                        if let Some(t) = &e.target {
                            if let Some(c) = next.filesystem.path_counts.get_mut(t) {
                                *c = c.saturating_sub(1);
                                if *c == 0 {
                                    next.filesystem.path_counts.remove(t);
                                }
                            }
                        }
                    }
                    "window.launch" => {
                        if let Some(app) = &e.target {
                            next.windows.push(WindowInfo {
                                title: app.clone(),
                                app: app.clone(),
                            });
                        }
                    }
                    "window.close" => {
                        if let Some(app) = &e.target {
                            if let Some(pos) = next.windows.iter().position(|w| &w.app == app) {
                                next.windows.remove(pos);
                            }
                        }
                    }
                    "browser.tab" => {
                        if let Some(url) = &e.target {
                            next.browser_tabs.push(url.clone());
                        }
                    }
                    "browser.close" => {
                        if let Some(url) = &e.target {
                            if let Some(pos) = next.browser_tabs.iter().position(|t| t == url) {
                                next.browser_tabs.remove(pos);
                            }
                        }
                    }
                    "clipboard.set" => {
                        next.clipboard = e.target.clone();
                    }
                    _ => {}
                }
            }
        }
        next
    }
}

/// The rich simulation contract every service implements.
pub trait Simulatable: Send + Sync {
    /// What must already be true for the action to be valid.
    fn preconditions(&self, req: &SimRequest, world: &WorldState) -> Vec<Precondition>;

    /// Structured side effects of the action (side-effect graph).
    fn side_effects(&self, req: &SimRequest) -> Vec<SideEffect>;

    /// Per-step cost/quality estimate.
    fn estimate(&self, req: &SimRequest) -> StepEstimate;

    /// Resource forecast contributed by the action.
    fn forecast(&self, req: &SimRequest) -> ResourceForecast;

    /// How the action could be undone, if at all.
    fn rollback(&self, req: &SimRequest) -> Option<RollbackPlan>;

    /// Bundle the above into a single [`SimResult`]. Default impl; override only if
    /// a cheaper joint computation is possible.
    fn simulate(&self, req: &SimRequest, world: &WorldState) -> SimResult {
        SimResult {
            effects: self.side_effects(req),
            forecast: self.forecast(req),
            estimate: self.estimate(req),
            preconditions: self.preconditions(req, world),
            rollback: self.rollback(req),
        }
    }
}
