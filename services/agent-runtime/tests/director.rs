//! Agent Director integration tests: the supervisor topology — one parent
//! spawns bounded, role-typed children (Researcher / Analyst / Verifier),
//! collects them, and only trusts the run when every result passes the gate.
//! All drivers are deterministic closures; the real `SkillAgentDriver` face
//! is proven in `services/action-runtime/tests/agent_director.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use friday_agent_runtime::{
    default_gate, AgentAbort, AgentContext, AgentDirector, AgentDriver, AgentResult, AgentRole,
    AgentState, AgentStep, AsyncFnDriver, DirectorConfig, FnDriver, SecurityScope,
};

fn ok(id: &str, summary: &str) -> AgentResult {
    AgentResult::succeeded(id, summary, vec![AgentStep::ok(1, "docs.search", 2)])
}

/// A driver that succeeds for everyone.
fn ok_driver() -> Arc<dyn AgentDriver> {
    Arc::new(FnDriver::new(|ctx: AgentContext| {
        ok(&ctx.spec.id, "did the task")
    }))
}

#[tokio::test]
async fn director_runs_the_three_role_topology_and_verifies_every_result() {
    let director = AgentDirector::new(ok_driver());

    let researcher = director.spawn(AgentRole::Researcher, "find the documentation");
    let analyst = director.spawn(AgentRole::Analyst, "analyze the evidence");
    let verifier = director.spawn(AgentRole::Verifier, "verify the finding");

    let report = director
        .verify(vec![researcher, analyst, verifier], default_gate())
        .await;

    assert_eq!(report.entries.len(), 3);
    assert_eq!(report.succeeded(), 3);
    assert_eq!(report.verified(), 3);
    assert!(
        report.all_verified(),
        "{}",
        report.summary_lines().join("\n")
    );

    let roles: Vec<AgentRole> = report.entries.iter().map(|e| e.role).collect();
    assert_eq!(
        roles,
        vec![
            AgentRole::Researcher,
            AgentRole::Analyst,
            AgentRole::Verifier
        ]
    );
    assert!(report
        .entries
        .iter()
        .all(|e| e.result.agent_id.starts_with(&format!("{}.", e.role))));
}

#[tokio::test]
async fn failed_child_blocks_the_verified_outcome() {
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        if ctx.spec.id.starts_with("verifier.") {
            AgentResult::failed(&ctx.spec.id, "the check could not run", vec![], "no skill")
        } else {
            ok(&ctx.spec.id, "did the task")
        }
    }));
    let director = AgentDirector::new(driver);

    let researcher = director.spawn(AgentRole::Researcher, "find the documentation");
    let verifier = director.spawn(AgentRole::Verifier, "verify the finding");

    let report = director
        .verify(vec![researcher, verifier], default_gate())
        .await;

    assert_eq!(report.verified(), 1);
    assert!(
        !report.all_verified(),
        "a failed verifier must block the run: {}",
        report.summary_lines().join("\n")
    );
    let verifier_entry = report
        .entries
        .iter()
        .find(|e| e.role == AgentRole::Verifier)
        .expect("verifier entry");
    assert!(!verifier_entry.verified);
    assert!(
        verifier_entry
            .verdict
            .as_deref()
            .unwrap_or_default()
            .contains("ended failed"),
        "{:?}",
        verifier_entry.verdict
    );
}

#[tokio::test]
async fn cancelled_child_is_not_verified() {
    let driver: Arc<dyn AgentDriver> = Arc::new(AsyncFnDriver::new(|ctx: AgentContext| {
        Box::pin(async move {
            ctx.cancellation.cancelled().await;
            AgentResult::aborted(&ctx.spec.id, AgentAbort::Cancelled, "cancelled", vec![], 0)
        })
    }));
    let director = AgentDirector::new(driver);

    let agents = vec![
        director.spawn(AgentRole::Researcher, "wait"),
        director.spawn(AgentRole::Verifier, "verify"),
    ];
    director.cancel(&agents);

    let report = director.verify(agents, default_gate()).await;

    assert!(!report.all_verified());
    assert_eq!(report.verified(), 0);
    assert!(
        report
            .entries
            .iter()
            .all(|e| e.result.state == AgentState::Cancelled),
        "{:?}",
        report.entries
    );
}

#[tokio::test]
async fn failed_child_report_can_be_reassigned_and_verified() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let first_attempt = Arc::clone(&attempts);
    let failing_driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(move |ctx: AgentContext| {
        first_attempt.fetch_add(1, Ordering::SeqCst);
        AgentResult::failed(
            &ctx.spec.id,
            "worker lost its provider",
            vec![],
            "provider offline",
        )
    }));
    let first_director = AgentDirector::new(failing_driver);
    let failed = first_director
        .verify(
            vec![first_director.spawn(AgentRole::Researcher, "research safely")],
            default_gate(),
        )
        .await;

    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(!failed.all_verified());
    assert_eq!(failed.entries.len(), 1);
    assert_eq!(failed.entries[0].result.state, AgentState::Failed);

    // Reassignment is explicit and bounded: the parent retains the failed
    // report, then creates one replacement child under a fresh director/driver.
    let replacement_driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        ok(&ctx.spec.id, "replacement completed the task")
    }));
    let replacement_director = AgentDirector::new(replacement_driver);
    let replacement = replacement_director
        .verify(
            vec![replacement_director.spawn(AgentRole::Researcher, "research safely")],
            default_gate(),
        )
        .await;
    assert!(
        replacement.all_verified(),
        "{}",
        replacement.summary_lines().join("\\n")
    );
    assert_eq!(replacement.verified(), 1);
}

#[tokio::test]
async fn verifier_with_no_steps_is_not_verified() {
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        // A verifier that "succeeded" without running a single step.
        AgentResult::succeeded(&ctx.spec.id, "all good", vec![])
    }));
    let director = AgentDirector::new(driver);

    let verifier = director.spawn(AgentRole::Verifier, "verify");
    let report = director.verify(vec![verifier], default_gate()).await;

    assert!(!report.all_verified());
    let entry = &report.entries[0];
    assert!(!entry.verified);
    assert!(
        entry
            .verdict
            .as_deref()
            .unwrap_or_default()
            .contains("without a single successful check"),
        "{:?}",
        entry.verdict
    );
}

#[tokio::test]
async fn director_bounds_concurrency() {
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let running2 = Arc::clone(&running);
    let peak2 = Arc::clone(&peak);

    let driver: Arc<dyn AgentDriver> = Arc::new(AsyncFnDriver::new(move |_ctx: AgentContext| {
        let running2 = Arc::clone(&running2);
        let peak2 = Arc::clone(&peak2);
        Box::pin(async move {
            let now = running2.fetch_add(1, Ordering::SeqCst) + 1;
            peak2.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            running2.fetch_sub(1, Ordering::SeqCst);
            ok("w", "done")
        })
    }));

    let director = AgentDirector::with_config(
        driver,
        DirectorConfig {
            max_concurrency: 2,
            ..DirectorConfig::default()
        },
    );
    let agents = director.spawn_many(AgentRole::Researcher, (0..5).map(|i| format!("goal {i}")));

    let report = director.supervise(agents).await;

    assert_eq!(report.agents.len(), 5);
    assert!(report.all_succeeded());
    assert_eq!(
        peak.load(Ordering::SeqCst),
        2,
        "the director must never run more than max_concurrency children at once"
    );
}

#[tokio::test]
async fn role_scopes_and_bounds_ride_the_spec() {
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        let spec = &ctx.spec;
        assert!(
            spec.id.starts_with("researcher."),
            "id must be role-typed: {}",
            spec.id
        );
        assert_eq!(
            spec.allowed_skills,
            vec!["docs".to_string(), "web".to_string(), "git".to_string()],
            "role default scope must ride the spec"
        );
        assert_eq!(spec.max_steps, 8, "director bounds must ride the spec");
        ok(&spec.id, "scope ok")
    }));

    let director = AgentDirector::with_config(
        driver,
        DirectorConfig {
            max_steps: 8,
            ..DirectorConfig::default()
        },
    );
    let agent = director.spawn(AgentRole::Researcher, "find the documentation");
    let report = director.verify(vec![agent], default_gate()).await;

    assert!(report.all_verified(), "{:?}", report.entries);
}

#[tokio::test]
async fn assign_overrides_the_role_scope() {
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        assert_eq!(ctx.spec.allowed_skills, vec!["git".to_string()]);
        ok(&ctx.spec.id, "scoped ok")
    }));
    let director = AgentDirector::new(driver);

    // A verifier told to use only `git` — narrower than the role default.
    let agent = director.assign(AgentRole::Verifier, "check the git status", &["git"]);
    let report = director.verify(vec![agent], default_gate()).await;

    assert!(report.all_verified(), "{:?}", report.entries);
    assert!(report.entries[0].result.agent_id.starts_with("verifier."));
}

#[tokio::test]
async fn child_security_scope_is_inherited_and_clamped() {
    let seen: Arc<std::sync::Mutex<Vec<(String, Option<SecurityScope>)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_driver: Arc<std::sync::Mutex<Vec<(String, Option<SecurityScope>)>>> =
        Arc::clone(&seen);
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(move |ctx: AgentContext| {
        seen_driver
            .lock()
            .expect("record lock")
            .push((ctx.spec.id.clone(), ctx.spec.security_scope.clone()));
        ok(&ctx.spec.id, "scope ok")
    }));

    let parent_scope = SecurityScope::new(&["web", "git"], &["provider/openrouter"]);
    let director = AgentDirector::with_config(
        driver,
        DirectorConfig {
            security_scope: Some(parent_scope.clone()),
            ..DirectorConfig::default()
        },
    );

    let inherited = director.spawn(AgentRole::Researcher, "inherit the parent scope");
    let requested_broader = friday_agent_runtime::AgentSpec::new("verifier.99", "narrow me")
        .with_security_scope(SecurityScope::new(
            &["web", "browser", "filesystem"],
            &["provider/openrouter", "vault/root"],
        ));
    let clamped = director.spawn_spec(AgentRole::Verifier, requested_broader);

    let report = director.verify(vec![inherited, clamped], default_gate()).await;
    assert!(report.all_verified(), "{:?}", report.entries);

    let seen = seen.lock().expect("record lock");
    assert_eq!(seen.len(), 2);

    let inherited_scope = seen
        .iter()
        .find(|(id, _)| id.starts_with("researcher."))
        .and_then(|(_, scope)| scope.clone())
        .expect("inherited scope recorded");
    assert_eq!(inherited_scope, parent_scope);

    let clamped_scope = seen
        .iter()
        .find(|(id, _)| id.starts_with("verifier."))
        .and_then(|(_, scope)| scope.clone())
        .expect("clamped scope recorded");
    assert!(clamped_scope.is_subset_of(&parent_scope));
    assert_eq!(clamped_scope.egress, vec!["web".to_string()]);
    assert_eq!(
        clamped_scope.secrets,
        vec!["provider/openrouter".to_string()]
    );
    assert!(!clamped_scope.egress.contains(&"browser".to_string()));
    assert!(!clamped_scope.secrets.contains(&"vault/root".to_string()));
}

#[tokio::test]
async fn supervise_merges_a_fan_out() {
    let director = AgentDirector::new(ok_driver());
    let agents = director.spawn_many(AgentRole::Researcher, ["one", "two"]);

    let report = director.supervise(agents).await;

    assert_eq!(report.agents.len(), 2);
    assert_eq!(report.succeeded, 2);
    assert!(report.all_succeeded());
}

#[tokio::test]
async fn terminate_stops_and_collects_every_child() {
    let driver: Arc<dyn AgentDriver> = Arc::new(AsyncFnDriver::new(|ctx: AgentContext| {
        Box::pin(async move {
            ctx.cancellation.cancelled().await;
            AgentResult::aborted(&ctx.spec.id, AgentAbort::Cancelled, "stopped", vec![], 0)
        })
    }));
    let director = AgentDirector::new(driver);
    let agents: Vec<_> = (0..3)
        .map(|_| director.spawn(AgentRole::Analyst, "analyze"))
        .collect();

    let results = director.terminate(agents).await;

    assert_eq!(results.len(), 3);
    assert!(
        results
            .iter()
            .all(|(_, r)| r.state == AgentState::Cancelled),
        "{results:?}"
    );
}

#[tokio::test]
async fn director_spec_face_carries_params_and_cwd() {
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        assert!(ctx.spec.params.contains_key("docs.search"));
        assert!(ctx.spec.cwd.is_some());
        ok(&ctx.spec.id, "params ok")
    }));
    let director = AgentDirector::new(driver);

    let spec = friday_agent_runtime::AgentSpec::new("verifier.1", "check the docs")
        .with_allowed_skills(&["docs"])
        .with_param("docs.search", serde_json::json!({ "root": "/tmp" }))
        .with_cwd(std::path::PathBuf::from("/tmp"));
    let agent = director.spawn_spec(AgentRole::Verifier, spec);
    let report = director.verify(vec![agent], default_gate()).await;

    assert!(report.all_verified(), "{:?}", report.entries);
}
