//! Recovery taxonomy for FRIDAY (Reliability Sprint).
//!
//! Failure is classified into a [`FailureCategory`] and answered by a
//! deterministic [`RecoveryAction`]. Recovery is modelled at the **action
//! level** (e.g. `Browser.Navigate`), not the service level, so a single
//! capability can have several distinct recovery trees depending on *why* it
//! failed.

use serde::{Deserialize, Serialize};

/// Formal lifecycle stage for a recovery policy (or the runtime as a whole).
///
/// Every new recovery policy is promoted **Prototype → Shadow → Limited →
/// Production → Qualified**; it only affects real executions once it has agreed
/// with actual outcomes in shadow mode. The runtime `RecoveryMode` selects how
/// far along that ladder the system currently operates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RecoveryMode {
    /// Recovery is computed but never acted on, never recorded.
    Off,
    /// Recovery is computed and *recorded* ("I would have chosen…") but does not
    /// affect execution. The default rollout stage.
    #[default]
    Shadow,
    /// Recovery is computed, recorded, and actively applied to real failures.
    Active,
}

impl RecoveryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecoveryMode::Off => "off",
            RecoveryMode::Shadow => "shadow",
            RecoveryMode::Active => "active",
        }
    }

    pub fn from_mode_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "off" => RecoveryMode::Off,
            "active" => RecoveryMode::Active,
            _ => RecoveryMode::Shadow,
        }
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    /// Estimated recovery cost in milliseconds (a prior, like confidence). Lets
    /// the planner later weigh **Fast Recovery** vs **Reliable Recovery** — a
    /// second dimension of resilience.
    pub fn cost_ms(&self) -> u64 {
        match self {
            RecoveryAction::Retry { attempts } => *attempts as u64 * 50,
            RecoveryAction::RestartService { .. } => 2_000,
            RecoveryAction::WaitAndRetry { ms } => ms + 200,
            RecoveryAction::UseProvider { .. } => 800,
            RecoveryAction::UseAlternateCapability { .. } => 500,
            RecoveryAction::RestoreSession => 1_000,
            RecoveryAction::CleanupAndRetry { .. } => 300,
            RecoveryAction::SuggestElevation => 0,
            RecoveryAction::AskUser => 0,
            RecoveryAction::Escalate => 0,
        }
    }

    /// Would this step resolve the failure given the *category* of failure and
    /// what is *currently available* (the set of capability ids with a live
    /// provider)?
    ///
    /// Per the Reliability discipline, not every action can paper over every
    /// failure:
    ///
    /// * **Reroute steps** (`UseProvider`, `UseAlternateCapability`) resolve iff
    ///   their target capability is currently available. These are the only
    ///   steps that legitimately heal a dead provider.
    /// * **Conditional steps** (`Retry`, `WaitAndRetry`, `RestartService`,
    ///   `RestoreSession`, `CleanupAndRetry`) resolve *only* for transient
    ///   categories (`Network`, `Timeout`, `Resource`) — e.g. an HTTP 429 you can
    ///   wait out, or a stale lock you can clear. They do **not** resolve a
    ///   `Provider`-offline or `Permission`-denied failure; retrying a dead
    ///   provider just burns time.
    /// * **Human/higher-authority steps** (`SuggestElevation`, `AskUser`,
    ///   `Escalate`) never auto-resolve; they hand off.
    pub fn resolves(
        &self,
        category: FailureCategory,
        available_caps: &std::collections::HashSet<String>,
    ) -> bool {
        match self {
            RecoveryAction::UseProvider { capability }
            | RecoveryAction::UseAlternateCapability { capability } => {
                available_caps.contains(capability)
            }
            RecoveryAction::Retry { .. }
            | RecoveryAction::WaitAndRetry { .. }
            | RecoveryAction::RestartService { .. }
            | RecoveryAction::RestoreSession
            | RecoveryAction::CleanupAndRetry { .. } => is_transient(category),
            RecoveryAction::SuggestElevation
            | RecoveryAction::AskUser
            | RecoveryAction::Escalate => false,
        }
    }
}

/// A failure is "transient" if a bounded retry/wait/restart could plausibly fix
/// it without external help (the resource is still fundamentally reachable).
pub fn is_transient(c: FailureCategory) -> bool {
    matches!(
        c,
        FailureCategory::Network | FailureCategory::Timeout | FailureCategory::Resource
    )
}
