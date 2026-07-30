use crate::common::{Action, Capability, ExecutionContext};
use crate::confidence::Confidence;
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

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
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

/// A verifier — checks one aspect of a tool result.
#[async_trait]
pub trait Verifier: Send + Sync {
    fn name(&self) -> &str;
    fn verify(&self, action: &Action, result: &ToolResult) -> Check;
}
