use crate::common::{Action, Capability, ExecutionContext};
use crate::confidence::Confidence;
use crate::evidence::is_evidence_action;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ── Tool ────────────────────────────────────────────────────────────

/// A tool — a capability implementation.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn capabilities(&self) -> Vec<Capability>;
    async fn execute(&self, action: &Action, context: &ExecutionContext) -> ToolResult;
}

/// Result of a tool execution (raw, unverified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub metadata: serde_json::Value,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
            duration_ms: 0,
            metadata: serde_json::json!({}),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(message.into()),
            duration_ms: 0,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

// ── Tool Registry ───────────────────────────────────────────────────

/// A registry of swappable tools. Resolves Capability → Best Tool.
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn resolve(&self, capability: &Capability) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.capabilities().contains(capability))
            .map(|t| t.as_ref())
    }

    /// Resolve the tool for an *action* rather than a bare capability.
    ///
    /// Evidence actions (`gather evidence: <channel> (<tool>.<verb>)`) resolve
    /// to their channel's evidence tool (`evidence:web`, `evidence:docs`, …)
    /// by name, so the investigation seam keeps its normalized / health-gated
    /// semantics even when a dedicated goal tool claims the same skill id (a
    /// learned workflow policy references `web.search` / `docs.search` as goal
    /// actions, and must not be refused by the evidence wrapper). Falls back
    /// to capability resolution when no evidence-named tool is registered
    /// (kernels that mock evidence tools by capability), and non-evidence
    /// actions always resolve by capability.
    pub fn resolve_action(&self, action: &Action) -> Option<&dyn Tool> {
        if is_evidence_action(action) {
            if let Capability::Custom(id) = &action.capability {
                if let Some((channel, _)) = id.split_once('.') {
                    if let Some(tool) = self.get(&format!("evidence:{channel}")) {
                        return Some(tool);
                    }
                }
            }
        }
        self.resolve(&action.capability)
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn capabilities(&self) -> Vec<Capability> {
        let mut caps: std::collections::HashSet<Capability> = std::collections::HashSet::new();
        for tool in &self.tools {
            for cap in tool.capabilities() {
                caps.insert(cap);
            }
        }
        caps.into_iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Verification ────────────────────────────────────────────────────

/// Verification — interprets tool output into a verified result.
pub struct Verification {
    verifiers: Vec<Box<dyn Verifier>>,
}

impl Verification {
    pub fn new() -> Self {
        Self {
            verifiers: Vec::new(),
        }
    }

    pub fn register(mut self, verifier: Box<dyn Verifier>) -> Self {
        self.verifiers.push(verifier);
        self
    }

    pub fn verify(&self, action: &Action, result: &ToolResult) -> VerifiedResult {
        let mut checks = Vec::new();
        let mut all_passed = true;

        for verifier in &self.verifiers {
            let check = verifier.verify(action, result);
            if !check.passed {
                all_passed = false;
            }
            checks.push(check);
        }

        let verified = if self.verifiers.is_empty() {
            result.success
        } else {
            all_passed
        };

        let confidence = if checks.is_empty() {
            Confidence::medium()
        } else {
            let passed = checks.iter().filter(|c| c.passed).count();
            Confidence::from_ratio(passed as u32, checks.len() as u32)
        };

        VerifiedResult {
            verified,
            tool_success: result.success,
            checks,
            confidence,
        }
    }
}

impl Default for Verification {
    fn default() -> Self {
        Self::new()
    }
}

/// A single verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The result of verification — tool output interpreted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedResult {
    pub verified: bool,
    pub tool_success: bool,
    pub checks: Vec<Check>,
    /// Confidence in the verification result.
    pub confidence: Confidence,
}

/// The **epistemic** state of an execution (OMH "Evidence Before Claims"):
/// FRIDAY may only *claim* an outcome it has observed and verified.
///
/// ```text
/// Prepared ──► Observed ──► Verified
///      │           │
///      │           └──► Failed       (tool error or verification failure)
///      └──► Cancelled                (never dispatched — action-runtime layer)
/// ```
///
/// Deliberately distinct from action-runtime's `LifecycleState`
/// (`Created/Running/Cancelled/Completed/Archived`): that is the **process**
/// lifecycle — what the scheduler did. `ExecutionState` is the **evidence**
/// lifecycle — what FRIDAY knows. One axis never implies the other: a
/// completed process can still be unverified, and a verified claim always
/// required an observed tool result first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionState {
    /// The action was intended (planned) but not yet dispatched.
    Prepared,
    /// A tool ran and produced raw output — observed, not yet interpreted.
    Observed,
    /// The observed output passed verification — success may be claimed only
    /// from this state.
    Verified,
    /// The action failed: tool error, no tool, or verification failure.
    Failed,
    /// Reserved for the action-runtime interrupt/resume layer: the action was
    /// cancelled before completing. The GoalEngine never produces this today.
    Cancelled,
}

impl ExecutionState {
    /// States a transition *from* `self` may legally target.
    pub fn next_allowed(&self) -> &'static [ExecutionState] {
        match self {
            Self::Prepared => &[Self::Observed, Self::Cancelled],
            Self::Observed => &[Self::Verified, Self::Failed],
            Self::Verified | Self::Failed | Self::Cancelled => &[],
        }
    }

    /// `true` once no further transition is legal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Verified | Self::Failed | Self::Cancelled)
    }

    /// Wire label used in the cognitive trace and outcome audit trail.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Observed => "observed",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The single place the evidence lifecycle is derived from a verification
    /// result. `Verified` requires *both* raw tool success and every
    /// verification check passing — a failed tool, a missing tool, or a
    /// verification failure all land on `Failed`. Every caller must use this
    /// so the rule cannot drift between sites.
    pub fn from_verified(verified: &VerifiedResult) -> Self {
        if verified.verified && verified.tool_success {
            Self::Verified
        } else {
            Self::Failed
        }
    }
}

/// A verifier — checks one aspect of a tool result.
#[async_trait]
pub trait Verifier: Send + Sync {
    fn name(&self) -> &str;
    fn verify(&self, action: &Action, result: &ToolResult) -> Check;
}
