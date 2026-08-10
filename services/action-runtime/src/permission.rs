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
//!
//! A second tier exists for **sensitive-by-default** actions
//! ([`PermissionEngine::with_default_deny`]): they are denied unless the
//! request carries an explicit `"approved": true` parameter, set by the
//! approval flow. This is the capability-scoped model from
//! `specs/browser-organ.md` §4 — e.g. downloads, uploads, and arbitrary page
//! JS are off unless the human explicitly says so.

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
    /// Substrings that are denied by default, unless the request carries an
    /// explicit `"approved": true` parameter (set by the approval flow).
    default_deny_contains: Vec<String>,
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
            default_deny_contains: Vec::new(),
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

    /// Add substrings that are **denied by default** — allowed only when the
    /// request carries an explicit `"approved": true` parameter.
    pub fn with_default_deny(
        mut self,
        patterns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        for p in patterns {
            self.default_deny_contains.push(p.into());
        }
        self
    }

    /// Seed the sensitive browser actions from `specs/browser-organ.md` §4:
    /// downloads, uploads, arbitrary page JS, purchases, payments, financial
    /// actions, and account settings are off unless explicitly approved.
    pub fn with_browser_sensitive_defaults(self) -> Self {
        self.with_default_deny([
            "web.download",
            "web.upload",
            "web.evaluate",
            "web.purchase",
            "web.checkout",
            "web.pay",
            "web.buy",
            "account_settings",
            "financial",
        ])
    }

    /// An action is explicitly approved when the caller set `"approved": true`
    /// on the request parameters (the approval flow's contract).
    fn explicitly_approved(req: &ActionRequest) -> bool {
        req.parameters
            .get("approved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
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
        if !Self::explicitly_approved(req)
            && self
                .default_deny_contains
                .iter()
                .any(|d| target.contains(d.as_str()))
        {
            tracing::warn!(
                action = %target,
                "permission denied by default — requires explicit approval"
            );
            return PermissionDecision::Deny;
        }
        self.default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browser_sensitive_actions_denied_by_default() {
        let engine = PermissionEngine::new().with_browser_sensitive_defaults();
        // Safe/medium browser verbs still auto-approve.
        assert_eq!(
            engine.decide(&ActionRequest::new("web", "open", json!({}))),
            PermissionDecision::AllowOnce
        );
        assert_eq!(
            engine.decide(&ActionRequest::new("web", "type", json!({}))),
            PermissionDecision::AllowOnce
        );
        // Sensitive verbs are denied unless approved.
        assert_eq!(
            engine.decide(&ActionRequest::new("web", "evaluate", json!({}))),
            PermissionDecision::Deny
        );
        assert_eq!(
            engine.decide(&ActionRequest::new("web", "download", json!({}))),
            PermissionDecision::Deny
        );
        assert_eq!(
            engine.decide(&ActionRequest::new("web", "purchase", json!({}))),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn explicit_approval_overrides_default_deny() {
        let engine = PermissionEngine::new().with_browser_sensitive_defaults();
        let approved = ActionRequest::new(
            "web",
            "evaluate",
            json!({ "approved": true, "expression": "1 + 1" }),
        );
        assert_eq!(engine.decide(&approved), PermissionDecision::AllowOnce);
        // Hard deny-list still wins even with approval.
        let still_denied = ActionRequest::new("web", "delete_all", json!({ "approved": true }));
        assert_eq!(engine.decide(&still_denied), PermissionDecision::Deny);
    }
}
