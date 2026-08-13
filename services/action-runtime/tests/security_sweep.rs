//! #12 security integration sweep — the security model *across seams*, not
//! per-component.
//!
//! The component slices (C1/C2/E, provider egress, scope clamping) are each
//! proven by their own suites. This file closes the gaps that only show up
//! when seams meet:
//!
//! ```text
//! 1. Provider          env credential → SecretsVault → EgressGate → provider
//! 2. Parent → child    parent SecurityScope → AgentDirector → child scope
//! 3. Kernel → Web      SecurityScope → kernel → WebAdapter → redirect → EgressPolicy
//! 4. Kernel → Browser  SecurityScope → kernel → BrowserAdapter → CDP Fetch → EgressPolicy
//! 5. Child → network   parent authority → child authority → network action → same boundary
//! 6. Secrets + egress  secret-carrying request → inspection → deny → audit reason only
//! ```
//!
//! Coverage status going in:
//!
//! - **Seam 1** is proven end-to-end by the intelligence suite
//!   (`default_from_env_attaches_security_context_and_blocks_secret_payloads`):
//!   env → vault → gate attached → clean payload reaches the provider, a
//!   secret payload is blocked before dispatch, and the routing decision
//!   records the block without the value. Not duplicated here.
//! - **Seam 2** is proven by the agent-runtime suites
//!   (`child_security_scope_is_inherited_and_clamped`,
//!   `workforce_run_inherits_the_parent_security_scope`). This file adds the
//!   missing half: that the clamped scope is then **enforced**.
//! - **Seam 3** cannot be run as one hermetic end-to-end test on purpose: the
//!   kernel's pre-dispatch gate refuses every internal initial URL
//!   unconditionally, so no loopback mock server can ever feed a real
//!   `WebAdapter` through the kernel, and no real non-internal host is
//!   reachable hermetically. The seam is covered by composition — the kernel
//!   gate over the initial URL (`tests/egress.rs`), the adapter's hop
//!   re-check over a real redirect (`tests/egress_redirect.rs`), and the
//!   kernel's terminal handling of the adapter's `egress_deny` contract
//!   (`kernel_audits_redirect_denial_cancels_and_does_not_retry`).
//! - **Seams 4 + 6** through the real kernel are exercised live here
//!   (`kernel_audits_browser_redirect_denial_and_cancels`, in the browser
//!   adapter's test module so Chrome launches stay serialized), plus a
//!   hermetic proof that the kernel owns the initial URL for the browser
//!   seam too.
//!
//! The invariant this suite guards: **security authority has exactly one
//! owner.** [`SecurityScope`] defines what an execution may do; child scopes
//! only become narrower; adapters enforce the policy; the kernel owns the
//! authoritative audit/cancellation decision. No adapter or caller quietly
//! invents its own authority — the kernel overwrites whatever scope a request
//! carried with the context's stamped authority at dispatch.

use async_trait::async_trait;
use friday_actions::kernel::{ExecutionContext, LifecycleState};
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::runtime::AgentKernel;
use friday_actions::{ActionAdapter, BrowserAdapter};
use friday_agent_runtime::{
    AgentContext, AgentDirector, AgentDriver, AgentResult, AgentRole, AgentSpec, AsyncFnDriver,
    DirectorConfig, SecurityScope,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn fetch(url: &str) -> ActionRequest {
    ActionRequest::new("web", "fetch", json!({ "url": url }))
}

fn open(url: &str) -> ActionRequest {
    ActionRequest::new("web", "open", json!({ "url": url }))
}

fn ctx(scope: Option<SecurityScope>) -> ExecutionContext {
    ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .security_scope(scope)
        .build()
}

/// A `web.fetch` adapter that only records whether it was reached and what
/// scope the kernel stamped onto the request. It never touches the network, so
/// "the adapter was called" is a positive signal that the kernel's gate let the
/// action through — and the recorded scope is proof of which authority the
/// adapter actually enforced.
struct RecordingWebAdapter {
    calls: Arc<AtomicUsize>,
    scopes: Arc<Mutex<Vec<Option<SecurityScope>>>>,
}

#[async_trait]
impl ActionAdapter for RecordingWebAdapter {
    fn tool(&self) -> &str {
        "web"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["fetch"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.scopes.lock().unwrap().push(req.security_scope.clone());
        ActionResult::success(json!({ "ok": true }))
    }
}

/// The single-owner invariant: the kernel stamps the authority onto every
/// dispatched request and discards whatever scope a caller smuggled in. A
/// caller (or a misbehaving adapter) can neither widen nor invent authority —
/// the adapter only ever sees the context's scope.
#[tokio::test]
async fn kernel_stamps_authority_over_request_carried_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let scopes: Arc<Mutex<Vec<Option<SecurityScope>>>> = Arc::new(Mutex::new(Vec::new()));
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(RecordingWebAdapter {
            calls: Arc::clone(&calls),
            scopes: Arc::clone(&scopes),
        }))
        .await;

    // A request that claims a broad scope is dispatched with the context's
    // authority — here, none. The kernel, not the request, decides.
    let mut req = fetch("https://example.com/x");
    req.security_scope = Some(SecurityScope::new(&["example.com", "evil.example"], &[]));
    let res = kernel.run_action(req, ctx(None)).await;
    assert!(
        res.success,
        "unscoped external dispatch keeps legacy behavior"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        scopes.lock().unwrap()[0],
        None,
        "the adapter must see the kernel's authority, not the request's"
    );

    // A narrow context authority clamps a request that claims more.
    let mut req = fetch("https://example.com/x");
    req.security_scope = Some(SecurityScope::new(&["evil.example", "example.com"], &[]));
    let narrow = SecurityScope::new(&["example.com"], &[]);
    let res = kernel.run_action(req, ctx(Some(narrow.clone()))).await;
    assert!(res.success);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        scopes.lock().unwrap()[1],
        Some(narrow),
        "the context's narrower authority replaces the request's claim"
    );
}

/// Parent → child → network (seams 2 + 5): the director clamps a child that
/// requests broader authority, and the *clamped* scope is what the kernel
/// enforces when the child performs a network action. A child reaching outside
/// the clamped authority is refused at the kernel's pre-dispatch gate (the
/// adapter never runs); a child staying inside it flows through the very same
/// boundary. Nothing the child claimed, requested, or carried can widen the
/// authority the parent granted.
#[tokio::test]
async fn child_scope_clamped_by_the_director_is_enforced_by_the_kernel() {
    let calls = Arc::new(AtomicUsize::new(0));
    let scopes: Arc<Mutex<Vec<Option<SecurityScope>>>> = Arc::new(Mutex::new(Vec::new()));
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(RecordingWebAdapter {
            calls: Arc::clone(&calls),
            scopes: Arc::clone(&scopes),
        }))
        .await;

    // A child driver that mirrors the production bridge (agent_driver.rs):
    // the worker's clamped spec scope becomes the ExecutionContext authority,
    // and the action runs through the same kernel every action flows through.
    let child_kernel = Arc::clone(&kernel);
    let driver: Arc<dyn AgentDriver> = Arc::new(AsyncFnDriver::new(move |ctx: AgentContext| {
        let kernel = Arc::clone(&child_kernel);
        Box::pin(async move {
            let spec = ctx.spec.clone();
            let observed_scope = spec.security_scope.clone();
            let url = spec
                .params
                .get("web.fetch")
                .and_then(|p| p.get("url"))
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let req = ActionRequest::new("web", "fetch", json!({ "url": url }));
            let exec_ctx = ExecutionContext::builder()
                .execution_id(uuid::Uuid::new_v4())
                .session_id(uuid::Uuid::new_v4())
                .security_scope(spec.security_scope.clone())
                .build();
            let execution_id = exec_ctx.execution_id;
            let result = kernel.run_action(req, exec_ctx).await;
            let base = if result.success {
                AgentResult::succeeded(spec.id.clone(), format!("fetch {url} ok"), vec![])
            } else {
                AgentResult::failed(
                    spec.id.clone(),
                    format!("fetch {url} refused"),
                    vec![],
                    result.error.clone().unwrap_or_default(),
                )
            };
            base.with_observation(json!({
                "scope": observed_scope,
                "success": result.success,
                "error": result.error,
                "execution_id": execution_id.to_string(),
            }))
        })
    }));

    let parent_scope = SecurityScope::new(&["friday.test"], &["provider/openrouter"]);
    let director = AgentDirector::with_config(
        driver,
        DirectorConfig {
            security_scope: Some(parent_scope.clone()),
            ..DirectorConfig::default()
        },
    );

    // The denied child asks for BROADER authority than the parent grants and
    // reaches outside it.
    let denied_child = AgentSpec::new("denied", "reach outside the parent authority")
        .with_security_scope(SecurityScope::new(
            &["friday.test", "evil.example"],
            &["provider/openrouter", "vault/root"],
        ))
        .with_param("web.fetch", json!({ "url": "https://evil.example/x" }));
    // The allowed child stays inside the parent's authority.
    let allowed_child = AgentSpec::new("allowed", "stay inside the parent authority")
        .with_security_scope(SecurityScope::new(
            &["friday.test"],
            &["provider/openrouter"],
        ))
        .with_param("web.fetch", json!({ "url": "https://friday.test/x" }));

    let report = director
        .supervise(vec![
            director.spawn_spec(AgentRole::Verifier, denied_child),
            director.spawn_spec(AgentRole::Verifier, allowed_child),
        ])
        .await;

    let denied = report
        .agents
        .iter()
        .find(|a| a.agent_id == "denied")
        .expect("denied child recorded");
    let allowed = report
        .agents
        .iter()
        .find(|a| a.agent_id == "allowed")
        .expect("allowed child recorded");

    // Both children were handed exactly the clamped scope — never the broader
    // one the denied child requested.
    for agent in [&denied, &allowed] {
        let obs = agent.observation.as_ref().expect("driver observed scope");
        let seen: SecurityScope = serde_json::from_value(obs["scope"].clone()).expect("scope json");
        assert!(seen.is_subset_of(&parent_scope), "clamped subset: {seen:?}");
        assert_eq!(seen.egress, vec!["friday.test".to_string()]);
        assert_eq!(seen.secrets, vec!["provider/openrouter".to_string()]);
        assert!(!seen.egress.contains(&"evil.example".to_string()));
        assert!(!seen.secrets.contains(&"vault/root".to_string()));
    }

    // The out-of-scope child was refused by the kernel before the adapter ran,
    // and the kernel cancelled its execution.
    assert!(!denied.state.is_success(), "{denied:?}");
    let denied_obs = denied.observation.as_ref().unwrap();
    assert_eq!(
        denied_obs["success"], false,
        "out-of-scope fetch must be refused"
    );
    assert_eq!(denied_obs["error"], "egress denied");
    let denied_exec: &str = denied_obs["execution_id"].as_str().unwrap();
    assert_eq!(
        kernel
            .kernel()
            .lifecycle
            .state(uuid::Uuid::parse_str(denied_exec).unwrap()),
        Some(LifecycleState::Cancelled)
    );
    let entries = kernel.audit().recent(10);
    let denied_audit = entries
        .iter()
        .find(|e| e.decision == "EgressDeny")
        .expect("an EgressDeny audit entry must exist");
    assert_eq!(
        denied_audit.detail.as_deref(),
        Some("denied by egress policy (destination_not_covered_by_scope)")
    );

    // The in-scope child flowed through the same boundary to the adapter.
    assert!(allowed.state.is_success(), "{allowed:?}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "only the in-scope fetch reached the adapter"
    );
}

/// Kernel → Browser, hermetic half (seam 4): the kernel owns the initial URL
/// for the browser seam exactly as it does for every other network action. An
/// out-of-scope `web.open` is refused at the pre-dispatch gate — the browser
/// never launches, and the denial is the kernel's own (no adapter `egress_deny`
/// redirect contract involved).
#[tokio::test]
async fn kernel_denies_out_of_scope_browser_open_before_the_browser_launches() {
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(BrowserAdapter::new()))
        .await;

    let c = ctx(Some(SecurityScope::new(&["friday.test"], &[])));
    let execution_id = c.execution_id;
    let req = open("https://evil.example/page");
    let res = kernel.run_action(req, c).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert!(
        !res.metadata.contains_key("egress_deny"),
        "a pre-dispatch denial is the kernel's own gate, not the adapter's redirect contract"
    );
    assert_eq!(
        kernel.kernel().lifecycle.state(execution_id),
        Some(LifecycleState::Cancelled)
    );
    let entries = kernel.audit().recent(10);
    let denied = entries
        .iter()
        .find(|e| e.decision == "EgressDeny")
        .expect("an EgressDeny audit entry must exist");
    assert_eq!(
        denied.detail.as_deref(),
        Some("denied by egress policy (destination_not_covered_by_scope)")
    );
}
