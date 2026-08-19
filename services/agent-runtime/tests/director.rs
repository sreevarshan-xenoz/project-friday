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

    let report = director
        .verify(vec![inherited, clamped], default_gate())
        .await;
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

// ── Phase 5: AgentDirector cognitive trace hierarchy ───────────────────

#[tokio::test]
async fn verify_emits_hierarchical_trace_tree() {
    use friday_agent_runtime::{
        CognitiveTraceContext, InMemoryTraceCollector, TraceId, SessionId, GoalId,
    };
    use friday_core::trace::reconstruct_trace_tree;
    use uuid::Uuid;

    let collector = InMemoryTraceCollector::new();
    let trace_id = TraceId::from(Uuid::new_v4());
    let session_id = SessionId::from(Uuid::new_v4());
    let goal_id = GoalId::from(Uuid::new_v4());
    let trace_ctx = CognitiveTraceContext::for_goal(trace_id, session_id, goal_id);

    let director = AgentDirector::new(ok_driver())
        .with_tracer(Arc::new(collector.clone()))
        .with_trace_ctx(trace_ctx);

    let researcher = director.spawn(AgentRole::Researcher, "find the docs");
    let analyst = director.spawn(AgentRole::Analyst, "analyze the evidence");
    let verifier = director.spawn(AgentRole::Verifier, "verify the finding");

    let report = director
        .verify(vec![researcher, analyst, verifier], default_gate())
        .await;
    assert!(report.all_verified());

    // ── Assert trace hierarchy ───────────────────────────────────────
    let traces = collector.traces().clone();
    let tree = reconstruct_trace_tree(&traces);

    // Exactly one root span: director_supervise.
    assert_eq!(tree.len(), 1, "expected exactly one root span");
    let root = &tree[0];
    assert_eq!(root.trace.operation, "director_supervise");
    assert_eq!(root.trace.organ, "agent-director");
    assert_eq!(
        root.trace.result,
        friday_core::trace::ResultType::Success
    );

    // Root has 3 agent_supervise children.
    assert_eq!(root.children.len(), 3, "expected 3 agent children");

    // Each agent child has exactly 1 verification_gate child.
    for child in &root.children {
        assert_eq!(child.trace.operation, "agent_supervise");
        assert_eq!(child.trace.organ, "agent-director");
        assert_eq!(
            child.trace.result,
            friday_core::trace::ResultType::Success
        );

        let meta = &child.trace.metadata;
        assert!(meta.get("agent.role").is_some(), "missing agent.role");
        assert!(meta.get("agent.agent_id").is_some(), "missing agent.agent_id");
        assert!(meta.get("agent.goal").is_some(), "missing agent.goal");
        assert_eq!(meta["agent.verified"], true);

        // Each agent exposes its actual work (Phase 5D): a tool_call step
        // plus the verification gate.
        let gate = child
            .children
            .iter()
            .find(|c| c.trace.operation == "verification_gate")
            .expect("expected a verification_gate child per agent");
        assert_eq!(gate.trace.operation, "verification_gate");
        assert_eq!(gate.trace.organ, "agent-director");
        assert!(
            child
                .children
                .iter()
                .any(|c| c.trace.operation == "tool_call"),
            "expected a tool_call step child"
        );
        assert_eq!(
            gate.trace.result,
            friday_core::trace::ResultType::Success
        );
        assert!(gate.children.is_empty(), "gate span should have no children");
    }

    // All spans share the same trace_id.
    for t in &traces {
        assert_eq!(t.trace_id, trace_id, "trace_id mismatch on {}", t.operation);
    }

    // Root has no parent; agent and gate spans do.
    assert!(root.trace.parent_span_id.is_none());
    for child in &root.children {
        assert!(child.trace.parent_span_id.is_some(), "agent span missing parent");
        assert_eq!(child.children[0].trace.parent_span_id, Some(child.trace.span_id));
    }
}

#[tokio::test]
async fn verify_failure_path_emits_failure_traces() {
    use friday_agent_runtime::{
        CognitiveTraceContext, InMemoryTraceCollector, TraceId, SessionId, GoalId,
    };
    use friday_core::trace::reconstruct_trace_tree;
    use uuid::Uuid;

    let collector = InMemoryTraceCollector::new();
    let trace_id = TraceId::from(Uuid::new_v4());
    let trace_ctx = CognitiveTraceContext::for_goal(
        trace_id,
        SessionId::from(Uuid::new_v4()),
        GoalId::from(Uuid::new_v4()),
    );

    // A driver that always fails.
    let failing_driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(|ctx: AgentContext| {
        AgentResult::failed(&ctx.spec.id, "task failed", vec![], "internal error")
    }));

    let director = AgentDirector::new(failing_driver)
        .with_tracer(Arc::new(collector.clone()))
        .with_trace_ctx(trace_ctx);

    let researcher = director.spawn(AgentRole::Researcher, "find something");

    let report = director.verify(vec![researcher], default_gate()).await;
    assert!(!report.all_verified());

    let traces = collector.traces().clone();
    let tree = reconstruct_trace_tree(&traces);

    // Root span is Failure.
    let root = &tree[0];
    assert_eq!(root.trace.operation, "director_supervise");
    assert_eq!(root.trace.result, friday_core::trace::ResultType::Failure);

    // Agent span is Failure.
    let agent = &root.children[0];
    assert_eq!(agent.trace.operation, "agent_supervise");
    assert_eq!(agent.trace.result, friday_core::trace::ResultType::Failure);

    // Gate span is Failure.
    let gate = &agent.children[0];
    assert_eq!(gate.trace.operation, "verification_gate");
    assert_eq!(gate.trace.result, friday_core::trace::ResultType::Failure);

    // Gate verdict metadata indicates why.
    let gate_meta = &gate.trace.metadata;
    assert_eq!(gate_meta["gate.verified"], false);
    let verdict = gate_meta["gate.verdict"].as_str().unwrap();
    assert!(!verdict.is_empty(), "gate should explain the rejection");
}

// ── Phase 5E: full cognitive episode reconstruction ──────────────────
//
// The golden test for FRIDAY cognitive observability. One shared collector
// carries the entire episode — Intent → Planner (create_plan + its own
// llm_call) → Director → three parallel agents (Researcher/Analyst/Verifier),
// each with model/tool steps → verification_gate. The Director receives the
// Planner's propagated `TraceContext` (5C), so the whole thing is ONE trace,
// not a correlated-but-separate forest.
//
// The test is the authority on parentage: it reconstructs the tree and asserts
// identity, topology, cognitive ordering, real telemetry, and failure
// semantics. Execution order of the parallel agents is intentionally NOT
// asserted — only that all three parent to the Director.

#[tokio::test]
async fn full_cognitive_episode_trace() {
    use friday_agent_runtime::{
        AgentDirector, AgentResult, AgentRole, AgentStep, CognitiveTraceContext, GoalId,
        InMemoryTraceCollector, SessionId, SpanId, TraceId,
    };
    use friday_core::trace::{reconstruct_trace_tree, ModelUsage, TraceCollector, TraceTreeNode};
    use uuid::Uuid;

    let collector = InMemoryTraceCollector::new();
    let trace_id = TraceId::from(Uuid::new_v4());
    let session_id = SessionId::from(Uuid::new_v4());
    let goal_id = GoalId::from(Uuid::new_v4());

    // ── Planner (using the same CognitiveTraceContext primitives the real
    //    planner engine uses) ──
    let intent_ctx = CognitiveTraceContext::for_goal(trace_id, session_id, goal_id);
    let intent_span = SpanId::from(Uuid::new_v4());
    collector.record(intent_ctx.trace(intent_span, "intent", "classify"));

    let planner_ctx = intent_ctx.child(intent_span);
    let planner_span = SpanId::from(Uuid::new_v4());
    collector.record(planner_ctx.trace(planner_span, "planner", "create_plan"));

    // The planner's own model call (the LLM planner). Carries real telemetry.
    let planner_llm_span = SpanId::from(Uuid::new_v4());
    let planner_usage = ModelUsage {
        model: "gemma-3-27b".into(),
        provider: "ollama".into(),
        tokens_in: Some(1200),
        tokens_out: Some(340),
        cached_tokens: None,
        reasoning_tokens: None,
        cost_usd: Some(0.0021),
        finish_reason: Some("stop".into()),
    };
    collector.record(
        planner_ctx
            .child(planner_span)
            .trace(planner_llm_span, "planner", "llm_call")
            .with_model_usage(planner_usage),
    );

    // ── Propagate the Planner's context to the Director (5C boundary:
    //    parent_span_id == planner_span) ──
    let director_ctx = planner_ctx.child(planner_span);

    // Deterministic per-role driver: Researcher reasons + acts, Analyst acts,
    // Verifier deliberately fails its check (failure semantics).
    let model_usage = ModelUsage {
        model: "gemma-3-12b".into(),
        provider: "ollama".into(),
        tokens_in: Some(820),
        tokens_out: Some(190),
        cached_tokens: None,
        reasoning_tokens: None,
        cost_usd: Some(0.0011),
        finish_reason: Some("stop".into()),
    };
    let driver: Arc<dyn AgentDriver> = Arc::new(FnDriver::new(move |ctx: AgentContext| {
        let role = ctx.spec.id.split('.').next().unwrap_or("");
        match role {
            "researcher" => AgentResult::succeeded(
                &ctx.spec.id,
                "researched the topic",
                vec![
                    AgentStep::model(1, "reasoning.route", 12)
                        .with_model_usage(model_usage.clone()),
                    AgentStep::ok(2, "web.search", 34),
                ],
            ),
            "analyst" => AgentResult::succeeded(
                &ctx.spec.id,
                "analyzed the evidence",
                vec![AgentStep::ok(1, "document.read", 18)],
            ),
            // Deliberately failing verification: a failed tool step means the
            // Verifier has no successful check, so it cannot be verified.
            "verifier" => AgentResult::succeeded(
                &ctx.spec.id,
                "verification ran",
                vec![AgentStep::failed(1, "testing.check", "assertion failed", 5)],
            ),
            _ => AgentResult::succeeded(&ctx.spec.id, "ok", vec![]),
        }
    }));

    let director = AgentDirector::new(driver)
        .with_tracer(Arc::new(collector.clone()))
        .with_trace_ctx(director_ctx);

    // Three parallel agents (spawned together, collected concurrently).
    let researcher = director.spawn(AgentRole::Researcher, "research the topic");
    let analyst = director.spawn(AgentRole::Analyst, "analyze evidence");
    let verifier = director.spawn(AgentRole::Verifier, "verify the finding");
    let report = director
        .verify(vec![researcher, analyst, verifier], default_gate())
        .await;

    let traces = collector.traces().clone();
    let tree = reconstruct_trace_tree(&traces);

    // Helpers ----------------------------------------------------------
    let by_op = |op: &str| traces.iter().find(|t| t.operation == op).expect("missing span");
    let by_role = |role: &str| {
        traces
            .iter()
            .find(|t| {
                t.operation == "agent_supervise"
                    && t.metadata["agent.role"].as_str() == Some(role)
            })
            .expect("missing agent span")
    };
    let span_set: std::collections::HashSet<SpanId> =
        traces.iter().map(|t| t.span_id).collect();

    // ── 1. Identity ──
    for t in &traces {
        assert_eq!(t.trace_id, trace_id, "trace_id mismatch on {}", t.operation);
    }
    assert_eq!(span_set.len(), traces.len(), "span_ids must be unique");

    // ── 2. Topology ──
    assert_eq!(tree.len(), 1, "expected exactly one root (one trace)");
    // No orphan / dangling parent references.
    for t in &traces {
        if let Some(p) = t.parent_span_id {
            assert!(span_set.contains(&p), "dangling parent on {}", t.operation);
        }
    }
    // Exactly one root span.
    assert_eq!(
        traces.iter().filter(|t| t.parent_span_id.is_none()).count(),
        1,
        "expected exactly one root span"
    );

    // ── 3. Cognitive ordering (parentage) ──
    let intent = by_op("classify");
    let planner = by_op("create_plan");
    let director_span = by_op("director_supervise");
    assert!(intent.parent_span_id.is_none(), "intent is the root");
    assert_eq!(planner.parent_span_id, Some(intent.span_id));
    assert_eq!(director_span.parent_span_id, Some(planner.span_id), "director is child of planner");

    for role in ["researcher", "analyst", "verifier"] {
        let agent = by_role(role);
        assert_eq!(
            agent.parent_span_id,
            Some(director_span.span_id),
            "{role} must parent to the director (parallel-safe)"
        );
        // Its steps and verification gate parent to the agent.
        let gate = traces
            .iter()
            .find(|t| {
                t.operation == "verification_gate"
                    && t.parent_span_id == Some(agent.span_id)
            })
            .expect("agent missing gate");
        assert_eq!(gate.parent_span_id, Some(agent.span_id));
        let step = traces
            .iter()
            .find(|t| {
                (t.operation == "llm_call" || t.operation == "tool_call")
                    && t.parent_span_id == Some(agent.span_id)
            })
            .expect("agent missing a step span");
        assert_eq!(step.parent_span_id, Some(agent.span_id));
    }

    // ── 4. Real telemetry ──
    // Both discriminators present.
    assert!(
        traces.iter().any(|t| t.operation == "llm_call"),
        "expected a model (llm_call) span"
    );
    assert!(
        traces.iter().any(|t| t.operation == "tool_call"),
        "expected a tool (tool_call) span"
    );
    // A model span carries real inference telemetry.
    let model_span = traces
        .iter()
        .find(|t| t.operation == "llm_call" && t.model_usage.is_some())
        .expect("expected a model span with usage");
    let mu = model_span.model_usage.as_ref().unwrap();
    assert!(!mu.model.is_empty(), "model name missing");
    assert!(!mu.provider.is_empty(), "provider missing");
    assert!(mu.tokens_in.is_some(), "tokens_in missing");
    assert!(mu.tokens_out.is_some(), "tokens_out missing");
    assert!(mu.cost_usd.is_some(), "cost missing");
    // A tool span carries capability / outcome / duration.
    let tool_span = traces
        .iter()
        .find(|t| t.operation == "tool_call")
        .expect("expected a tool span");
    assert_eq!(tool_span.metadata["step.capability"], "web.search");
    assert_eq!(tool_span.metadata["step.outcome"], "ok");
    assert!(tool_span.duration_ms > 0, "tool duration missing");

    // ── 5. Failure semantics ──
    // The verifier's step failed, so it is NOT verified, and that failure is
    // not silently converted into verification success.
    assert!(!report.all_verified(), "a failing verifier must not verify");
    let verifier_agent = by_role("verifier");
    assert_eq!(
        verifier_agent.result,
        friday_core::trace::ResultType::Failure,
        "failed verifier agent must be Failure"
    );
    let verifier_gate = traces
        .iter()
        .find(|t| {
            t.operation == "verification_gate"
                && t.parent_span_id == Some(verifier_agent.span_id)
        })
        .expect("verifier missing gate");
    assert_eq!(
        verifier_gate.result,
        friday_core::trace::ResultType::Failure,
        "verifier gate must be Failure"
    );
    let verifier_step = traces
        .iter()
        .find(|t| {
            t.operation == "tool_call" && t.parent_span_id == Some(verifier_agent.span_id)
        })
        .expect("verifier missing step");
    assert_eq!(
        verifier_step.result,
        friday_core::trace::ResultType::Failure,
        "failed tool step must be Failure"
    );

    // Sanity: the tree is fully connected (flatten == flat list size).
    fn count(nodes: &[TraceTreeNode], n: &mut usize) {
        for node in nodes {
            *n += 1;
            count(&node.children, n);
        }
    }
    let mut total = 0usize;
    count(&tree, &mut total);
    assert_eq!(total, traces.len(), "every span must appear in the tree");
}
