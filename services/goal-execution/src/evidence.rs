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
//!
//! The same seam serves the **Browser Organ**: `gather evidence: external
//! (webagent.ask)` routes to the `webagent` adapter and runs against the
//! dedicated logged-in browser profile — research goals can ask ChatGPT / Grok
//! / Gemini through the browser, no API keys (see `specs/browser-organ.md`).
//! The planner's evidence actions always carry `{"query": <goal>}`; the
//! webagent adapter treats that as the message and defaults the site to
//! `FRIDAY_WEBAGENT_SITE` (else `https://chatgpt.com`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use friday_actions::{ActionAdapter, ActionRequest};
use serde::{Deserialize, Serialize};

use crate::common::{Action, Capability, ExecutionContext};
use crate::verification::{ExecutionState, Tool, ToolRegistry, ToolResult};

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

/// Whether an action is an evidence action (`gather evidence: … (<cap_id>)`).
///
/// Two independent conditions must both hold: the instruction carries the
/// `gather evidence: … (<cap_id>)` shape **and** the capability is a
/// `Capability::Custom` id. The planner emits exactly this combination for
/// every required channel, while the goal's own final action carries an
/// inferred *built-in* capability — so a goal description that happens to
/// contain `(word.word)` can never be misclassified as evidence.
pub fn is_evidence_action(action: &Action) -> bool {
    parse_evidence_id(&action.instruction).is_some()
        && matches!(action.capability, Capability::Custom(_))
}

/// One evidence action's epistemic lifecycle record: the Prepared → Observed
/// → Verified | Failed trail for a single `gather evidence:` action, as driven
/// by the `GoalEngine`. Every evidence action in an evidence-gated plan gets
/// its own record — the audit can answer "did *this* channel's evidence
/// verify?" per channel, not just for the plan as a whole.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceActionRecord {
    /// Position in the plan (0-based).
    pub step: usize,
    /// The full instruction, e.g. `gather evidence: git (git.status)`.
    pub instruction: String,
    /// The real skill capability id, e.g. `git.status`.
    pub capability: String,
    /// The capability prefix / adapter routing key (the tool part of
    /// `<tool>.<verb>`), e.g. `git`, `web` — not the `InvestigationChannel`
    /// label, though they coincide for every channel today.
    pub channel: String,
    /// The final epistemic state of this evidence action.
    pub execution_state: ExecutionState,
    pub verified: bool,
    pub tool_success: bool,
    pub duration_ms: u64,
}

/// The evidence trail of one evidence-gated execution: every evidence action
/// in the plan carrying its own Prepared → Observed → Verified | Failed
/// lifecycle. Published by the executive to the workspace so observers (the
/// bridge's live trace) can render each channel's epistemic state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceTrail {
    /// The goal whose evidence-gated plan produced this trail.
    pub goal: String,
    /// One record per evidence action, in plan order.
    pub records: Vec<EvidenceActionRecord>,
}

impl EvidenceTrail {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when every evidence action ended `Verified` (success may be
    /// claimed only through the evidence lifecycle).
    pub fn all_verified(&self) -> bool {
        !self.records.is_empty()
            && self
                .records
                .iter()
                .all(|r| r.execution_state == ExecutionState::Verified)
    }

    pub fn verified_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.execution_state == ExecutionState::Verified)
            .count()
    }
}

/// Register one [`AdapterTool`] per adapter into a [`ToolRegistry`].
///
/// This is the production wiring seam for evidence channels: the bridge and
/// the daemon both construct their adapter set here. The WebAgent adapter
/// (`webagent.ask/send/read` — the External channel's logged-in sites) belongs
/// in the same list as git and web.
///
/// ```rust
/// use friday_actions::{GitAdapter, WebAdapter, WebAgentAdapter};
/// use friday_goal_execution::{register_evidence_adapters, ToolRegistry};
/// use std::sync::Arc;
///
/// let mut registry = ToolRegistry::new();
/// register_evidence_adapters(
///     &mut registry,
///     &[
///         Arc::new(GitAdapter::new()),
///         Arc::new(WebAdapter::new()),
///         Arc::new(WebAgentAdapter::new()),
///     ],
/// );
/// assert!(registry.get("evidence:git").is_some());
/// assert!(registry.get("evidence:web").is_some());
/// assert!(registry.get("evidence:webagent").is_some());
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

    // ── Evidence trail helpers ───────────────────────────────────────

    #[test]
    fn is_evidence_action_detects_gather_evidence_shape() {
        let evidence = Action::single(
            "gather evidence: git (git.status)",
            Capability::Custom("git.status".into()),
            "g1",
        );
        assert!(is_evidence_action(&evidence));

        let goal_action = Action::single("run cargo test", Capability::ExecuteCommand, "g1");
        assert!(!is_evidence_action(&goal_action));
        assert!(!is_evidence_action(&Action::single(
            "gather evidence: git (no-dot)",
            Capability::Custom("git.status".into()),
            "g1"
        )));
        // The instruction shape alone is not enough: a goal description that
        // happens to contain `(word.word)` with a *built-in* capability must
        // not be misclassified as evidence.
        assert!(!is_evidence_action(&Action::single(
            "Check (docs.rs) for details",
            Capability::ExecuteCommand,
            "g1"
        )));
    }

    #[test]
    fn evidence_trail_counts_verified_and_gates() {
        let record = |state: ExecutionState| EvidenceActionRecord {
            step: 0,
            instruction: "gather evidence: git (git.status)".into(),
            capability: "git.status".into(),
            channel: "git".into(),
            execution_state: state,
            verified: state == ExecutionState::Verified,
            tool_success: state == ExecutionState::Verified,
            duration_ms: 5,
        };

        let empty = EvidenceTrail::default();
        assert!(empty.is_empty());
        assert!(!empty.all_verified(), "empty trail is never all-verified");

        let mut trail = EvidenceTrail {
            goal: "g1".into(),
            records: vec![record(ExecutionState::Verified)],
        };
        assert!(!trail.is_empty());
        assert_eq!(trail.len(), 1);
        assert!(trail.all_verified());
        assert_eq!(trail.verified_count(), 1);

        // A single failed evidence action breaks the gate.
        trail.records.push(record(ExecutionState::Failed));
        assert!(!trail.all_verified());
        assert_eq!(trail.verified_count(), 1);
    }

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

    // ── WebAgent evidence: gather evidence: external (webagent.ask) ────

    /// Live browser integration (`FRIDAY_BROWSER_TEST=1`): an evidence action
    /// for the External channel runs the Browser Organ end-to-end — open →
    /// type → submit → read — against a local echo page, using exactly the
    /// planner's parameter contract (`{"query": <goal>}`) and no explicit
    /// site.
    #[tokio::test]
    async fn webagent_evidence_runs_real_browser_roundtrip() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!(
                "skipping webagent evidence integration (set FRIDAY_BROWSER_TEST=1 to enable)"
            );
            return;
        }
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let base = serve_html_loop(html).await;

        let tool = AdapterTool::for_evidence(Arc::new(
            friday_actions::WebAgentAdapter::new()
                .with_profile(std::env::temp_dir().join("friday-webagent-evidence-test"))
                .with_default_site(base),
        ));
        let action = Action::single(
            "gather evidence: external (webagent.ask)",
            Capability::Custom("webagent.ask".into()),
            "g1",
        )
        .with_parameters(json!({ "query": "Hello evidence" }));
        let ctx = ExecutionContext::for_goal("g1");
        let result = tool.execute(&action, &ctx).await;
        assert!(
            result.success,
            "webagent evidence failed: {:?}",
            result.error
        );
        assert!(
            result.output.contains("Hello evidence"),
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

    /// Serve one canned HTML page on an ephemeral **loop** server — every
    /// request gets the same body. The browser organ fetches the page and may
    /// make follow-up requests (e.g. `/favicon.ico`), so the server must not
    /// exit after the first connection.
    async fn serve_html_loop(html: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
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
            }
        });
        format!("http://{addr}/")
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
