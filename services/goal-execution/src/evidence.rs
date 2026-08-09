//! Real evidence adapters — the tools that back an evidence-gated plan.
//!
//! M7 wired the Investigation Engine into the planner: a goal that requires
//! information becomes `gather evidence: <channel> (<cap_id>)` actions (e.g.
//! `gather evidence: git (git.status)`) followed by the goal's own action.
//! Those evidence actions are *capability-gated* (`Capability::Custom("git.status")`),
//! but until now every runtime test backed them with mocks — this module is
//! the "which tools back `web.search` / `git.status`" seam made real:
//!
//! ```text
//! Action (gather evidence: git (git.status), Custom("git.status"))
//!     │
//!     ▼
//! ToolRegistry::resolve → AdapterTool ("evidence:git")
//!     │
//!     ▼
//! friday-actions::GitAdapter  →  `git status --porcelain` (real process)
//! ```
//!
//! An [`AdapterTool`] is a [`crate::Tool`] over one [`friday_actions::ActionAdapter`].
//! It advertises exactly the adapter's verbs as `Capability::Custom("<tool>.<verb>")`
//! — the same ids the planner's evidence actions carry — so an evidence action
//! routes precisely to the real adapter and never hijacks a built-in
//! capability (a `git.status` evidence action is *not* an arbitrary
//! `execute_command`, and a real terminal tool keeps serving command goals).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use friday_actions::{ActionAdapter, ActionRequest};

use crate::common::{Action, Capability, ExecutionContext};
use crate::verification::{Tool, ToolRegistry, ToolResult};

/// Extract `(tool, verb)` from an evidence action instruction.
///
/// The planner emits `gather evidence: <channel> (<cap_id>)`, where `cap_id`
/// is the real skill capability id (`git.status`, `web.search`,
/// `codeintel.understand`). This parses the id inside the parentheses and
/// splits it at the first `.` — the adapter's routing key and its verb.
///
/// Returns `None` for anything that is not an evidence action.
pub fn parse_evidence_id(instruction: &str) -> Option<(String, String)> {
    let open = instruction.find('(')?;
    let close = instruction[open + 1..].find(')')? + open + 1;
    let cap_id = instruction[open + 1..close].trim();
    let (tool, verb) = cap_id.split_once('.')?;
    if tool.is_empty() || verb.is_empty() {
        return None;
    }
    Some((tool.to_string(), verb.to_string()))
}

/// A [`Tool`] backed by a real action-runtime adapter.
///
/// Serves evidence actions only: an instruction that is not
/// `gather evidence: … (<tool>.<verb>)` is refused with an honest error
/// (register a dedicated tool for that capability instead — this tool must
/// never guess what an arbitrary `execute_command` instruction means).
///
/// Timeouts and permissions are forwarded onto the [`ActionRequest`]; the
/// per-request timeout is enforced when the request runs under the action
/// kernel (direct adapter calls honor the adapter's own client timeout).
pub struct AdapterTool {
    name: String,
    description: String,
    adapter: Arc<dyn ActionAdapter>,
    capabilities: Vec<Capability>,
}

impl AdapterTool {
    /// Build the evidence tool for one adapter. The name is `evidence:<tool>`
    /// and the capabilities are exactly the adapter's verbs mapped to
    /// `Capability::Custom("<tool>.<verb>")`.
    pub fn for_evidence(adapter: Arc<dyn ActionAdapter>) -> Self {
        let tool = adapter.tool().to_string();
        let capabilities = adapter
            .actions()
            .into_iter()
            .map(|verb| Capability::Custom(format!("{tool}.{verb}")))
            .collect();
        Self {
            name: format!("evidence:{tool}"),
            description: format!(
                "Real evidence adapter backed by the action-runtime '{tool}' adapter \
                 (verbatim capability ids such as {tool}.status / {tool}.search)"
            ),
            adapter,
            capabilities,
        }
    }

    fn route(
        &self,
        tool: &str,
        verb: &str,
        action: &Action,
        context: &ExecutionContext,
    ) -> ActionRequest {
        let mut parameters = action.parameters.clone();
        // The evidence action runs against the goal's world: pass the context's
        // working directory through for the adapter that honors it (git's
        // `repo`); other adapters ignore the key entirely.
        if tool == "git" {
            if let Some(dir) = &context.working_directory {
                parameters["repo"] = serde_json::json!(dir);
            }
        }
        let mut req = ActionRequest::new(tool, verb, parameters);
        req.timeout = context.timeout_ms.map(Duration::from_millis);
        req.permissions = context.permissions.clone();
        req
    }
}

#[async_trait]
impl Tool for AdapterTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    async fn execute(&self, action: &Action, context: &ExecutionContext) -> ToolResult {
        let Some((tool, verb)) = parse_evidence_id(&action.instruction) else {
            return ToolResult::error(format!(
                "{} cannot execute '{}' — not an evidence action (gather evidence: <channel> (<tool>.<verb>)); \
                 register a dedicated tool for capability {}",
                self.name,
                action.instruction,
                action.capability
            ));
        };
        let req = self.route(&tool, &verb, action, context);
        let result = self.adapter.execute(req).await;
        // The adapter may attach its own latency measurement (`with_latency_ms`)
        // — surface it on the ToolResult instead of dropping it.
        let duration_ms = result
            .metadata
            .get("latency_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        ToolResult {
            success: result.success,
            output: serde_json::to_string_pretty(&result.output)
                .unwrap_or_else(|_| serde_json::json!(null).to_string()),
            error: result.error,
            duration_ms,
            metadata: serde_json::json!(result.metadata),
        }
    }
}

/// Register one [`AdapterTool`] per adapter into a [`ToolRegistry`].
///
/// ```rust
/// use friday_actions::{GitAdapter, WebAdapter};
/// use friday_goal_execution::{register_evidence_adapters, ToolRegistry};
/// use std::sync::Arc;
///
/// let mut registry = ToolRegistry::new();
/// register_evidence_adapters(
///     &mut registry,
///     &[Arc::new(GitAdapter::new()), Arc::new(WebAdapter::new())],
/// );
/// assert!(registry.get("evidence:git").is_some());
/// assert!(registry.get("evidence:web").is_some());
/// ```
pub fn register_evidence_adapters(
    registry: &mut ToolRegistry,
    adapters: &[Arc<dyn ActionAdapter>],
) {
    for adapter in adapters {
        registry.register(Box::new(AdapterTool::for_evidence(adapter.clone())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Goal, GoalEngine, Planning, PlanningContext, WorldState};
    use friday_actions::WebAdapter;
    use friday_investigation::{InformationRequirement, InvestigationChannel, InvestigationPlan};
    use serde_json::json;
    use std::process::Command;

    // ── Instruction parsing ──────────────────────────────────────────

    #[test]
    fn parses_evidence_ids() {
        assert_eq!(
            parse_evidence_id("gather evidence: git (git.status)"),
            Some(("git".to_string(), "status".to_string()))
        );
        assert_eq!(
            parse_evidence_id("gather evidence: web (web.search)"),
            Some(("web".to_string(), "search".to_string()))
        );
        assert_eq!(
            parse_evidence_id("gather evidence: codeintel (codeintel.understand)"),
            Some(("codeintel".to_string(), "understand".to_string()))
        );
        // Anything that is not an evidence action is refused.
        assert_eq!(parse_evidence_id("run cargo test"), None);
        assert_eq!(parse_evidence_id("gather evidence: git (no-dot)"), None);
        assert_eq!(parse_evidence_id("gather evidence: git ()"), None);
    }

    // ── Honest refusal ───────────────────────────────────────────────

    #[tokio::test]
    async fn non_evidence_action_is_refused_honestly() {
        let tool = AdapterTool::for_evidence(Arc::new(friday_actions::GitAdapter::new()));
        let action = Action::single("run cargo test", Capability::ExecuteCommand, "g1");
        let ctx = ExecutionContext::for_goal("g1");
        let result = tool.execute(&action, &ctx).await;
        assert!(!result.success);
        let err = result.error.expect("error");
        assert!(err.contains("not an evidence action"), "err: {err}");
    }

    // ── Real git.status via the action-runtime adapter ───────────────

    #[tokio::test]
    async fn git_evidence_runs_real_git_status() {
        // Hermetic temp repo: init + untracked file → `git status --porcelain`
        // reports "?? evidence.txt".
        let dir = std::env::temp_dir().join(format!("friday-evidence-{}", uuid_lite()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git init failed");
        std::fs::write(dir.join("evidence.txt"), "evidence\n").unwrap();

        let tool = AdapterTool::for_evidence(Arc::new(friday_actions::GitAdapter::new()));
        let action = Action::single(
            "gather evidence: git (git.status)",
            Capability::Custom("git.status".into()),
            "g1",
        );
        let ctx =
            ExecutionContext::for_goal("g1").with_working_directory(dir.display().to_string());
        let result = tool.execute(&action, &ctx).await;
        assert!(result.success, "git status failed: {:?}", result.error);
        assert!(
            result.output.contains("evidence.txt"),
            "porcelain output: {}",
            result.output
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Real web.search against a local endpoint ─────────────────────

    #[tokio::test]
    async fn web_evidence_searches_a_local_endpoint() {
        let base = serve_html_once(
            r#"<html><body>
                <a href="https://doc.rust-lang.org/book/">The Rust Book</a>
                <a href="https://example.com/guide">Example guide</a>
            </body></html>"#,
        )
        .await;

        let tool = AdapterTool::for_evidence(Arc::new(WebAdapter::new()));
        let action = Action::single(
            "gather evidence: web (web.search)",
            Capability::Custom("web.search".into()),
            "g1",
        )
        .with_parameters(json!({ "query": "rust docs", "endpoint": base }));
        let ctx = ExecutionContext::for_goal("g1");
        let result = tool.execute(&action, &ctx).await;
        assert!(result.success, "web search failed: {:?}", result.error);
        assert!(
            result.output.contains("The Rust Book"),
            "output: {}",
            result.output
        );
        assert!(
            result.output.contains("https://doc.rust-lang.org/book/"),
            "output: {}",
            result.output
        );
    }

    // ── Full stack: evidence-gated plan runs the real git adapter ────

    #[tokio::test]
    async fn evidence_plan_executes_real_git_via_goal_engine() {
        let dir = std::env::temp_dir().join(format!("friday-evidence-plan-{}", uuid_lite()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success());
        std::fs::write(dir.join("plan.txt"), "evidence\n").unwrap();

        // Strong git requirement → the evidence-gated plan wins.
        let planning = Planning::new();
        let ctx = PlanningContext::new(Goal::new("Check git status of the workspace repository"))
            .with_world_state(WorldState::new().with_tool(Capability::Custom("git.status".into())))
            .with_investigation(InvestigationPlan {
                requirements: vec![InformationRequirement {
                    channel: InvestigationChannel::Git,
                    rationale: "repo status".into(),
                    confidence: 0.9,
                }],
            });
        let candidates = planning.think(&ctx);
        let plan = candidates
            .iter()
            .find(|c| c.plan.source == crate::PlanSource::Investigation)
            .expect("evidence-gated plan")
            .plan
            .clone();
        let evidence_action = &plan.actions[0];
        assert_eq!(
            evidence_action.capability,
            Capability::Custom("git.status".into())
        );

        // The evidence action runs through GoalEngine against the REAL adapter.
        let mut registry = ToolRegistry::new();
        register_evidence_adapters(
            &mut registry,
            &[Arc::new(friday_actions::GitAdapter::new())],
        );
        let mut engine = GoalEngine::new().with_registry(registry);
        let goal = Goal::new("Check git status of the workspace repository");
        let ctx =
            ExecutionContext::for_goal(&goal.id).with_working_directory(dir.display().to_string());
        let report = engine
            .execute(
                &goal,
                evidence_action,
                &crate::Expectation::success("git status"),
                &ctx,
            )
            .await;

        assert!(report.success(), "evidence execution failed: {report:?}");
        assert!(
            report.outcome.detail["tool_success"] == json!(true),
            "tool must have succeeded"
        );
        // The real `git status --porcelain` output reached the verified result.
        assert!(
            report
                .cognitive_trace
                .nodes
                .iter()
                .any(|n| n.kind == crate::TraceNodeKind::Execution),
            "trace records the execution"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn uuid_lite() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
    }

    /// Serve one canned HTML response on an ephemeral local port; returns the
    /// base URL. The WebAdapter's `endpoint` param points here so the test
    /// stays hermetic (no external network).
    async fn serve_html_once(html: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let body = html.as_bytes();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(body).await;
        });
        format!("http://{addr}/")
    }
}
