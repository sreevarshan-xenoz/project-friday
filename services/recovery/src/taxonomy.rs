//! Recovery taxonomy for FRIDAY (Reliability Sprint).
//!
//! Failure is classified into a [`FailureCategory`] and answered by a
//! deterministic [`RecoveryAction`]. Recovery is modelled at the **action
//! level** (e.g. `Browser.Navigate`), not the service level, so a single
//! capability can have several distinct recovery trees depending on *why* it
//! failed.

use serde::{Deserialize, Serialize};

/// The reason an action failed. Drives which recovery policy applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureCategory {
    Network,
    Capability,
    Permission,
    Resource,
    /// The owning provider is offline / unavailable (this is what Chaos Mode
    /// induces when it kills a provider).
    Provider,
    Timeout,
    Planner,
    Simulation,
    Execution,
    Unknown,
}

impl FailureCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureCategory::Network => "network",
            FailureCategory::Capability => "capability",
            FailureCategory::Permission => "permission",
            FailureCategory::Resource => "resource",
            FailureCategory::Provider => "provider",
            FailureCategory::Timeout => "timeout",
            FailureCategory::Planner => "planner",
            FailureCategory::Simulation => "simulation",
            FailureCategory::Execution => "execution",
            FailureCategory::Unknown => "unknown",
        }
    }

    /// Map a validation-issue `kind` (from the Simulation Engine) onto a
    /// failure category.
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "unavailable_service" => FailureCategory::Provider,
            "permission" => FailureCategory::Permission,
            "state" | "service" => FailureCategory::Simulation,
            "capability" => FailureCategory::Capability,
            _ => FailureCategory::Unknown,
        }
    }
}

/// A single, deterministic recovery step. Recovery is a *graph*: an ordered list
/// of these per (action, category), tried in order until one resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Retry the same action a bounded number of times (transient faults).
    Retry { attempts: u32 },
    /// Restart the owning provider/service, then retry.
    RestartService { provider: String },
    /// Wait and retry (e.g. lock contention, transient network).
    WaitAndRetry { ms: u64 },
    /// Reroute to another provider that advertises the *same* capability.
    UseProvider { capability: String },
    /// Achieve the goal via a *different* capability (e.g. Browser.Navigate ->
    /// Windows.OpenURL).
    UseAlternateCapability { capability: String },
    /// Restore a killed shell/session, then retry (Terminal).
    RestoreSession,
    /// Clean up a blocking artifact (e.g. stale git lock) and retry.
    CleanupAndRetry { cleanup: String },
    /// Surface an elevation prompt to the user (UAC / sudo).
    SuggestElevation,
    /// Hand the decision to the user.
    AskUser,
    /// Escalate to a higher authority (human operator / different subsystem).
    Escalate,
}

impl RecoveryAction {
    /// Base confidence that this step resolves the failure. Used for Recovery
    /// Confidence: low confidence -> the approval policy kicks in (AskUser).
    pub fn confidence(&self) -> f64 {
        match self {
            RecoveryAction::Retry { .. } => 0.70,
            RecoveryAction::RestartService { .. } => 0.80,
            RecoveryAction::WaitAndRetry { .. } => 0.70,
            RecoveryAction::UseProvider { .. } => 0.85,
            RecoveryAction::UseAlternateCapability { .. } => 0.90,
            RecoveryAction::RestoreSession => 0.80,
            RecoveryAction::CleanupAndRetry { .. } => 0.80,
            RecoveryAction::SuggestElevation => 0.60,
            RecoveryAction::AskUser => 0.95,
            RecoveryAction::Escalate => 0.90,
        }
    }

    /// Would this step resolve the failure given what is *currently available*
    /// (the set of capability ids with a live provider)?
    ///
    /// Steps that do not depend on another provider (retry, restart, wait,
    /// restore, cleanup, elevate, ask, escalate) always resolve. Reroute steps
    /// only resolve if their target capability is available.
    pub fn resolves(&self, available_caps: &std::collections::HashSet<String>) -> bool {
        match self {
            RecoveryAction::UseProvider { capability }
            | RecoveryAction::UseAlternateCapability { capability } => {
                available_caps.contains(capability)
            }
            _ => true,
        }
    }
}
