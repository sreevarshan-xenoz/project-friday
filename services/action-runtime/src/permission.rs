//! The Permission Engine: the single authority that decides whether an action
//! may touch the outside world.
//!
//! Plugins must **never** decide their own permissions — they only receive a
//! normalized request and return a result. Every request funnels through
//! [`PermissionEngine::decide`] here, before the runtime dispatches it. This is
//! what keeps the platform safe by construction (see `docs/Security.md`).
//!
//! Today the engine auto-approves (returns [`PermissionDecision::AllowOnce`])
//! so the pipeline is unblocked, but a hard deny list of dangerous capabilities
//! is always enforced regardless of policy, matching the forbidden actions in
//! the security spec (delete files, format, send mail, transfer money, install
//! software).

use crate::request::ActionRequest;
use serde::Serialize;

/// The four verdicts the permission engine can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
pub enum PermissionDecision {
    /// Approved for this single execution only.
    #[default]
    AllowOnce,
    /// Approved and remembered for future identical actions.
    AllowAlways,
    /// Blocked. The action is never dispatched.
    Deny,
    /// Approved but confined to a restricted sandbox (currently treated like
    /// `AllowOnce` with an audit flag; sandboxing is deepened in a later
    /// milestone).
    Sandbox,
}

/// Decides whether actions may run. Configure the deny list and default policy;
/// everything else is auto-approved for now.
pub struct PermissionEngine {
    /// Substrings that, if present in an action name, are always denied.
    deny_contains: Vec<String>,
    default: PermissionDecision,
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self {
            deny_contains: vec![
                "delete".to_string(),
                "format".to_string(),
                "send_mail".to_string(),
                "transfer".to_string(),
                "install".to_string(),
            ],
            default: PermissionDecision::AllowOnce,
        }
    }

    /// Override the default verdict for non-denied actions.
    pub fn with_default(mut self, decision: PermissionDecision) -> Self {
        self.default = decision;
        self
    }

    /// Add a substring that always forces a deny.
    pub fn deny_substring(mut self, s: impl Into<String>) -> Self {
        self.deny_contains.push(s.into());
        self
    }

    /// The single decision point for the runtime.
    pub fn decide(&self, req: &ActionRequest) -> PermissionDecision {
        let target = format!("{}.{}", req.tool, req.action).to_lowercase();
        if self
            .deny_contains
            .iter()
            .any(|d| target.contains(d.as_str()))
        {
            tracing::warn!(action = %target, "permission denied by deny-list");
            return PermissionDecision::Deny;
        }
        self.default
    }
}
