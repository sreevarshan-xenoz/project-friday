//! #12-C1 kernel-boundary egress acceptance tests.
//!
//! Prove the *kernel boundary* itself, not just the policy unit:
//!
//! - an allowed HTTPS destination passes the central egress check and reaches
//!   the adapter;
//! - localhost / `127.0.0.1` / RFC1918 / IPv6 local destinations are denied
//!   **before** adapter dispatch (adapter invocation count stays 0);
//! - an out-of-scope destination is denied before dispatch;
//! - a denied URL that embeds a secret produces an audited, redacted error —
//!   no secret (and no URL) in the error surface or the audit trail.

use async_trait::async_trait;
use friday_actions::egress::EgressPolicy;
use friday_actions::kernel::ExecutionContext;
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::runtime::AgentKernel;
use friday_actions::ActionAdapter;
use friday_agent_runtime::SecurityScope;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A `web` adapter that counts invocations instead of doing I/O, so tests can
/// assert that a denial happens before dispatch.
struct CountingAdapter {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ActionAdapter for CountingAdapter {
    fn tool(&self) -> &str {
        "web"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["fetch", "search", "open"]
    }

    async fn execute(&self, _req: ActionRequest) -> ActionResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ActionResult::success(json!({ "ok": true }))
    }
}

async fn kernel_with_web() -> (Arc<AgentKernel>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(CountingAdapter {
            calls: Arc::clone(&calls),
        }))
        .await;
    (kernel, calls)
}

fn ctx(scope: Option<SecurityScope>) -> ExecutionContext {
    ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .security_scope(scope)
        .build()
}

#[tokio::test]
async fn allowed_https_destination_reaches_the_adapter() {
    let (kernel, calls) = kernel_with_web().await;
    let scope = SecurityScope::new(&["example.com"], &[]);

    let req = ActionRequest::new("web", "fetch", json!({ "url": "https://example.com/docs" }));
    let res = kernel.run_action(req, ctx(Some(scope))).await;

    assert!(
        res.success,
        "allowed destination must dispatch: {:?}",
        res.error
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "adapter must run exactly once"
    );
}

#[tokio::test]
async fn internal_and_private_destinations_are_denied_before_adapter_dispatch() {
    for url in [
        "http://localhost/x",
        "http://127.0.0.1/x",
        "http://192.168.1.5/x",
        "http://10.0.0.1/x",
        "http://172.16.4.2/x",
        "http://[::1]/x",
    ] {
        let (kernel, calls) = kernel_with_web().await;
        // Even a scope that grants an external host cannot open internal ones.
        let scope = SecurityScope::new(&["example.com"], &[]);

        let req = ActionRequest::new("web", "fetch", json!({ "url": url }));
        let res = kernel.run_action(req, ctx(Some(scope))).await;

        assert!(!res.success, "internal URL must be denied: {url}");
        let err = res.error.unwrap_or_default();
        assert!(
            err.contains("egress"),
            "expected egress denial for {url}, got: {err}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "adapter must never be invoked for {url}"
        );
    }
}

#[tokio::test]
async fn out_of_scope_destination_is_denied_before_adapter_dispatch() {
    let (kernel, calls) = kernel_with_web().await;
    let scope = SecurityScope::new(&["example.com"], &[]);

    let req = ActionRequest::new(
        "web",
        "fetch",
        json!({ "url": "https://other-site.com/page" }),
    );
    let res = kernel.run_action(req, ctx(Some(scope))).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "denied request must not dispatch"
    );
}

#[tokio::test]
async fn denied_url_with_secret_leaks_nothing_into_audit_or_error() {
    let secret = "sk-friday-super-secret-4242";
    let url = format!("https://other-site.com/login?token={secret}");
    let (kernel, calls) = kernel_with_web().await;
    let scope = SecurityScope::new(&["example.com"], &[]);

    let req = ActionRequest::new("web", "fetch", json!({ "url": url }));
    let res = kernel.run_action(req, ctx(Some(scope))).await;

    assert!(!res.success);
    let err = res.error.unwrap_or_default();
    assert!(!err.contains(secret), "error leaked the secret: {err}");
    assert!(
        !err.contains("other-site.com"),
        "error leaked the url: {err}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let entries = kernel.audit().recent(10);
    let denied = entries
        .iter()
        .find(|e| e.decision == "EgressDeny")
        .expect("an EgressDeny audit entry must exist");
    let detail = denied.detail.clone().unwrap_or_default();
    assert!(
        !detail.contains(secret),
        "audit leaked the secret: {detail}"
    );
    assert!(
        !detail.contains("other-site.com"),
        "audit leaked the url: {detail}"
    );
}

#[tokio::test]
async fn search_endpoint_is_classified_against_the_scope() {
    let (kernel, calls) = kernel_with_web().await;
    let scope = SecurityScope::new(&["api.example.com"], &[]);

    let allowed = ActionRequest::new(
        "web",
        "search",
        json!({ "endpoint": "https://api.example.com/search", "query": "x" }),
    );
    let res = kernel.run_action(allowed, ctx(Some(scope.clone()))).await;
    assert!(
        res.success,
        "allowed search endpoint must dispatch: {:?}",
        res.error
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let internal = ActionRequest::new(
        "web",
        "search",
        json!({ "endpoint": "http://localhost/search", "query": "x" }),
    );
    let res = kernel.run_action(internal, ctx(Some(scope))).await;
    assert!(!res.success, "internal search endpoint must be denied");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no second dispatch");
}

#[tokio::test]
async fn default_search_endpoint_is_classified() {
    let (kernel, calls) = kernel_with_web().await;
    // No `endpoint` param → the adapter's default destination is classified.
    let scope = SecurityScope::new(&["html.duckduckgo.com"], &[]);

    let req = ActionRequest::new("web", "search", json!({ "query": "rust" }));
    let res = kernel.run_action(req, ctx(Some(scope))).await;

    assert!(
        res.success,
        "default search destination must dispatch: {:?}",
        res.error
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unscoped_external_destination_keeps_legacy_dispatch() {
    let (kernel, calls) = kernel_with_web().await;

    let req = ActionRequest::new("web", "fetch", json!({ "url": "https://example.com/x" }));
    let res = kernel.run_action(req, ctx(None)).await;

    assert!(
        res.success,
        "unscoped external request keeps legacy behavior: {:?}",
        res.error
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn hardened_policy_denies_unscoped_external_egress() {
    let calls = Arc::new(AtomicUsize::new(0));
    let kernel = Arc::new(
        AgentKernel::new()
            .with_timeout(Duration::from_secs(5))
            .with_egress(EgressPolicy::new().with_deny_without_scope(true)),
    );
    kernel
        .register_adapter(Box::new(CountingAdapter {
            calls: Arc::clone(&calls),
        }))
        .await;

    let req = ActionRequest::new("web", "fetch", json!({ "url": "https://example.com/x" }));
    let res = kernel.run_action(req, ctx(None)).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
