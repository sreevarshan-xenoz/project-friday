//! [`AgentDirector`] — the supervisor topology above [`AgentRuntime`].
//!
//! The runtime owns *how* a worker runs (spawn, lifecycle, cancellation,
//! deadlines, budgets, mailbox); the director owns *who runs, with what
//! bounds, and whether the outcome is trustworthy*. This is the missing
//! hierarchical layer between a goal and the workers that serve it:
//!
//! ```text
//!                  AgentDirector
//!                        │
//!          ┌─────────────┼─────────────┐
//!          ↓             ↓             ↓
//!      Researcher      Analyst      Verifier     ← role-typed, bounded workers
//!          │             │             │
//!          └─────────────┼─────────────┘
//!                        ↓
//!                verify → DirectorReport     (every child result gated)
//!                        ↓
//!             goal completes only if all_verified
//! ```
//!
//! Design rules:
//!
//! - **Roles are constraints, not implementations.** A role supplies a
//!   suggested skill scope and sane bounds; it never names a provider or an
//!   LLM. The actual worker behavior comes from the [`AgentDriver`] the
//!   caller supplies (production: `SkillAgentDriver` from `friday-actions`).
//! - **Bounded by construction.** Every child the director spawns inherits
//!   the pool cap, time limit, and step budget from [`DirectorConfig`] — a
//!   parent can never accidentally fan out without limits.
//! - **`Succeeded` is the only success.** [`AgentState`] is the trust floor;
//!   a child that failed, timed out, was cancelled, or hit a budget wall can
//!   never count as verified, no matter what its summary claims.
//! - **Verification is the parent's gate.** [`default_gate`] is deterministic
//!   (state + non-empty summary + verifier-ran-a-step); callers plug stronger
//!   checks in at the same seam (e.g. an evidence verifier or a code-test
//!   verifier behind the goal-execution `Verifier` trait).
//! - **No persistent workforce yet.** `suspend`/`resume` and an assignable
//!   task board belong to the persistent-workforce step; this layer proves the
//!   topology — one parent, bounded children, collected and verified results.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::driver::AgentDriver;
use crate::handle::AgentHandle;
use crate::merge::MergedReport;
use crate::runtime::AgentRuntime;
use crate::spec::{AgentResult, AgentSpec, AgentState, SecurityScope};

/// The first director topology: Researcher / Analyst / Verifier.
///
/// Serde uses the stable lowercase string form (`researcher`/`analyst`/
/// `verifier` — the same shape as [`AgentRole::as_str`]) so persisted
/// workforce records survive renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    /// Gathers information (docs, web, git).
    Researcher,
    /// Reasons over gathered information (code intelligence).
    Analyst,
    /// Proves a claim by running a check (tests, command runs, evidence).
    Verifier,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentRole::Researcher => "researcher",
            AgentRole::Analyst => "analyst",
            AgentRole::Verifier => "verifier",
        }
    }

    /// Suggested skill scope for the role — a *suggestion*, not a wall: the
    /// caller can assign a different scope with [`AgentDirector::assign`].
    pub fn default_skill_scope(&self) -> &'static [&'static str] {
        match self {
            AgentRole::Researcher => &["docs", "web", "git"],
            AgentRole::Analyst => &["codeintel"],
            AgentRole::Verifier => &["testing", "developer"],
        }
    }
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The bounds every child inherits. A director built from this config can
/// never fan out without limits — the runtime enforces the pool cap, the
/// per-worker deadline, and the step budget.
#[derive(Debug, Clone)]
pub struct DirectorConfig {
    /// Maximum workers running at once (runtime semaphore).
    pub max_concurrency: usize,
    /// Hard per-worker deadline (runtime timeout).
    pub time_limit: Duration,
    /// Per-worker step budget (0 = unlimited, inherited from the runtime).
    pub max_steps: u32,
    /// Security authority that children inherit unless a caller explicitly
    /// requests a narrower scope.
    pub security_scope: Option<SecurityScope>,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: crate::runtime::DEFAULT_MAX_CONCURRENCY,
            time_limit: crate::runtime::DEFAULT_TIME_LIMIT,
            max_steps: 16,
            security_scope: None,
        }
    }
}

/// A role-typed child: the role the parent assigned plus the runtime mailbox.
pub struct DirectedAgent {
    pub role: AgentRole,
    pub handle: AgentHandle,
}

impl std::fmt::Debug for DirectedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectedAgent")
            .field("role", &self.role)
            .field("id", &self.handle.id)
            .finish()
    }
}

/// A parent's trust verdict over one child result. `Ok(())` = verified.
pub type Verdict = Result<(), String>;

/// The parent's gate for trusting a child's result. Deterministic by default
/// ([`default_gate`]); a caller plugs real checks in at the same seam.
pub type ResultGate = Arc<dyn Fn(AgentRole, &AgentResult) -> Verdict + Send + Sync>;

/// One role-typed result after the gate has run.
#[derive(Debug)]
pub struct DirectorEntry {
    pub role: AgentRole,
    pub result: AgentResult,
    /// `true` when the gate returned `Ok(())`.
    pub verified: bool,
    /// The gate's rejection reason; `None` when verified.
    pub verdict: Option<String>,
}

/// The parent's view of a completed run: every child result with its verdict.
#[derive(Debug)]
pub struct DirectorReport {
    pub entries: Vec<DirectorEntry>,
    pub total_duration_ms: u64,
}

impl DirectorReport {
    /// Children that reached [`AgentState::Succeeded`].
    pub fn succeeded(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.result.state.is_success())
            .count()
    }

    /// Children that passed the parent's gate.
    pub fn verified(&self) -> usize {
        self.entries.iter().filter(|e| e.verified).count()
    }

    /// The gate the goal is gated on: at least one child ran **and** every
    /// child passed. A single unverified child blocks the whole run.
    pub fn all_verified(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.verified)
    }

    /// The structured observation of the first verified child — the payload
    /// the parent may fold into the world model. `None` unless the whole run
    /// is [`DirectorReport::all_verified`] (an unverified run proves nothing)
    /// and the verified child actually produced an observation.
    pub fn verified_observation(&self) -> Option<&serde_json::Value> {
        if !self.all_verified() {
            return None;
        }
        self.entries
            .first()
            .and_then(|entry| entry.result.observation.as_ref())
    }

    /// One human/planner-readable line per child.
    pub fn summary_lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| {
                format!(
                    "[{}] {}({}) — {} ({} step(s), {} ms{})",
                    if e.verified { "verified" } else { "unverified" },
                    e.role,
                    e.result.agent_id,
                    e.result.summary,
                    e.result.steps.len(),
                    e.result.duration_ms,
                    e.verdict
                        .as_ref()
                        .map(|v| format!(", gate: {v}"))
                        .unwrap_or_default(),
                )
            })
            .collect()
    }
}

/// The supervisor. Owns one runtime + one driver and spawns role-typed,
/// bounded children whose results the parent collects and verifies.
pub struct AgentDirector {
    runtime: AgentRuntime,
    driver: Arc<dyn AgentDriver>,
    config: DirectorConfig,
    next_id: AtomicU64,
}

impl AgentDirector {
    pub fn new(driver: Arc<dyn AgentDriver>) -> Self {
        Self::with_config(driver, DirectorConfig::default())
    }

    pub fn with_config(driver: Arc<dyn AgentDriver>, config: DirectorConfig) -> Self {
        Self {
            runtime: AgentRuntime::new()
                .with_max_concurrency(config.max_concurrency)
                .with_default_time_limit(config.time_limit),
            driver,
            config,
            next_id: AtomicU64::new(1),
        }
    }

    /// The underlying runtime (pool cap, deadlines, budgets live there).
    pub fn runtime(&self) -> &AgentRuntime {
        &self.runtime
    }

    pub fn config(&self) -> DirectorConfig {
        self.config.clone()
    }

    fn derive_security_scope(&self, requested: Option<&SecurityScope>) -> Option<SecurityScope> {
        match (self.config.security_scope.as_ref(), requested) {
            (None, None) => None,
            (None, Some(scope)) => Some(scope.clone()),
            (Some(parent), None) => Some(parent.clone()),
            (Some(parent), Some(scope)) => Some(parent.intersect(scope)),
        }
    }

    fn spec_for(&self, role: AgentRole, goal: &str) -> AgentSpec {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut spec = AgentSpec::new(format!("{}.{n}", role.as_str()), goal)
            .with_allowed_skills(role.default_skill_scope())
            .with_max_steps(self.config.max_steps);
        spec.security_scope = self.derive_security_scope(None);
        spec
    }

    /// **spawn** — a role-typed child with the role's default scope and the
    /// director's bounds.
    pub fn spawn(&self, role: AgentRole, goal: &str) -> DirectedAgent {
        let spec = self.spec_for(role, goal);
        DirectedAgent {
            role,
            handle: self.runtime.spawn(spec, Arc::clone(&self.driver)),
        }
    }

    /// **assign** — like [`AgentDirector::spawn`], but with an explicit skill
    /// scope instead of the role's suggestion (e.g. a verifier told to use
    /// only `git`).
    pub fn assign(&self, role: AgentRole, goal: &str, skill_scope: &[&str]) -> DirectedAgent {
        let mut spec = self.spec_for(role, goal);
        spec.allowed_skills = skill_scope.iter().map(|s| s.to_string()).collect();
        DirectedAgent {
            role,
            handle: self.runtime.spawn(spec, Arc::clone(&self.driver)),
        }
    }

    /// **spawn** a batch of same-role children (each respects the pool bound).
    pub fn spawn_many<I, S>(&self, role: AgentRole, goals: I) -> Vec<DirectedAgent>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        goals
            .into_iter()
            .map(|goal| self.spawn(role, &goal.into()))
            .collect()
    }

    /// The raw face for callers that need full spec control (params, cwd,
    /// explicit time limit). The role still tags the child for the gate.
    pub fn spawn_spec(&self, role: AgentRole, spec: AgentSpec) -> DirectedAgent {
        let mut spec = spec;
        spec.security_scope = self.derive_security_scope(spec.security_scope.as_ref());
        DirectedAgent {
            role,
            handle: self.runtime.spawn(spec, Arc::clone(&self.driver)),
        }
    }

    /// **supervise** — collect every child and fold the raw results into the
    /// runtime's [`MergedReport`] (the "did the fan-out succeed" view).
    pub async fn supervise(&self, agents: Vec<DirectedAgent>) -> MergedReport {
        let results = self.collect(agents).await;
        self.runtime
            .merge(results.into_iter().map(|(_, r)| r).collect())
    }

    /// **collect** — await every child, returning role-typed results.
    pub async fn collect(&self, agents: Vec<DirectedAgent>) -> Vec<(AgentRole, AgentResult)> {
        let mut out = Vec::with_capacity(agents.len());
        for agent in agents {
            let result = agent.handle.await_result().await;
            out.push((agent.role, result));
        }
        out
    }

    /// **cancel** — cooperative stop for every child (they stop at their next
    /// [`crate::context::AgentContext::check`]; the runtime hard-kills at the
    /// deadline regardless).
    pub fn cancel(&self, agents: &[DirectedAgent]) {
        for agent in agents {
            agent.handle.cancel();
        }
    }

    /// **terminate** — cancel and wait: the parent's forcible shutdown of a
    /// run. Returns whatever each child ended with (typically `Cancelled`).
    pub async fn terminate(&self, agents: Vec<DirectedAgent>) -> Vec<(AgentRole, AgentResult)> {
        self.cancel(&agents);
        self.collect(agents).await
    }

    /// **verify** — the money path. Await every child, run the parent's
    /// [`ResultGate`] over each result, and return the [`DirectorReport`].
    /// The goal may complete only when [`DirectorReport::all_verified`] is
    /// `true`.
    pub async fn verify(&self, agents: Vec<DirectedAgent>, gate: ResultGate) -> DirectorReport {
        let results = self.collect(agents).await;
        let mut entries = Vec::with_capacity(results.len());
        let mut total_duration_ms = 0u64;
        for (role, result) in results {
            total_duration_ms += result.duration_ms;
            let verdict = gate(role, &result);
            entries.push(DirectorEntry {
                role,
                verified: verdict.is_ok(),
                verdict: verdict.err(),
                result,
            });
        }
        DirectorReport {
            entries,
            total_duration_ms,
        }
    }
}

/// The deterministic default trust gate. A child is verified iff:
///
/// - it reached [`AgentState::Succeeded`] (failed / cancelled / timed out /
///   budget-exhausted children are never verified), and
/// - its summary is non-empty (a child that claims success with nothing to
///   show verifies nothing), and
/// - a **Verifier** additionally ran at least one **successful** check (`ok`
///   step outcome). A verifier whose steps all failed or were skipped
///   verified nothing and cannot be trusted.
pub fn default_gate() -> ResultGate {
    Arc::new(|role, result| {
        if result.state != AgentState::Succeeded {
            return Err(format!("child ended {}", result.state.as_str()));
        }
        if result.summary.trim().is_empty() {
            return Err("child succeeded with an empty summary".into());
        }
        if role == AgentRole::Verifier && !result.steps.iter().any(|s| s.outcome == "ok") {
            return Err("verifier reported success without a single successful check".into());
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok_result(id: &str) -> AgentResult {
        AgentResult::succeeded(
            id,
            "did the work",
            vec![crate::spec::AgentStep::ok(1, "docs.search", 2)],
        )
    }

    #[test]
    fn roles_have_stable_names_and_scopes() {
        assert_eq!(AgentRole::Researcher.as_str(), "researcher");
        assert_eq!(AgentRole::Analyst.as_str(), "analyst");
        assert_eq!(AgentRole::Verifier.as_str(), "verifier");
        assert_eq!(
            format!("{role}", role = AgentRole::Researcher),
            "researcher"
        );
        assert!(AgentRole::Researcher
            .default_skill_scope()
            .contains(&"docs"));
        assert!(AgentRole::Analyst
            .default_skill_scope()
            .contains(&"codeintel"));
        assert!(AgentRole::Verifier
            .default_skill_scope()
            .contains(&"testing"));
    }

    #[test]
    fn default_gate_accepts_a_succeeded_role_result() {
        let gate = default_gate();
        let mut ok = ok_result("researcher.1");
        ok.summary = "found the documentation".into();
        assert!(gate(AgentRole::Researcher, &ok).is_ok());
    }

    #[test]
    fn default_gate_rejects_non_succeeded_states() {
        let gate = default_gate();
        let failed = AgentResult::failed("verifier.2", "could not", vec![], "no skill");
        let verdict = gate(AgentRole::Verifier, &failed).unwrap_err();
        assert!(verdict.contains("ended failed"), "{verdict}");

        let timed_out = AgentResult::aborted(
            "researcher.3",
            crate::context::AgentAbort::TimedOut,
            "late",
            vec![],
            10,
        );
        assert!(gate(AgentRole::Researcher, &timed_out).is_err());
    }

    #[test]
    fn default_gate_rejects_empty_summaries() {
        let gate = default_gate();
        let empty = AgentResult::succeeded("analyst.1", "", vec![]);
        assert!(gate(AgentRole::Analyst, &empty).is_err());
    }

    #[test]
    fn default_gate_requires_a_verifier_to_have_run_a_successful_check() {
        let gate = default_gate();
        // A verifier that "succeeded" without running a single step verified
        // nothing — the parent must not trust it.
        let lazy = AgentResult::succeeded("verifier.1", "all good", vec![]);
        let verdict = gate(AgentRole::Verifier, &lazy).unwrap_err();
        assert!(
            verdict.contains("without a single successful check"),
            "{verdict}"
        );

        // Worse: a verifier whose steps all *failed* claims success but
        // verified nothing either.
        let all_failed = AgentResult::succeeded(
            "verifier.2",
            "all good",
            vec![crate::spec::AgentStep::failed(1, "git.status", "boom", 5)],
        );
        assert!(gate(AgentRole::Verifier, &all_failed).is_err());

        // A verifier with at least one successful check passes.
        let proven = AgentResult::succeeded(
            "verifier.3",
            "all good",
            vec![crate::spec::AgentStep::ok(1, "git.status", 5)],
        );
        assert!(gate(AgentRole::Verifier, &proven).is_ok());

        // Same empty result under a Researcher role is fine (researchers can
        // conclude without executing a capability).
        assert!(gate(AgentRole::Researcher, &lazy).is_ok());
    }

    #[test]
    fn report_verified_only_when_every_child_passes() {
        let mut ok = ok_result("researcher.1");
        ok.summary = "found it".into();

        let all_pass = DirectorReport {
            entries: vec![DirectorEntry {
                role: AgentRole::Researcher,
                verified: true,
                verdict: None,
                result: ok.clone(),
            }],
            total_duration_ms: 2,
        };
        assert!(all_pass.all_verified());
        assert_eq!(all_pass.verified(), 1);
        assert_eq!(all_pass.succeeded(), 1);

        let with_failure = DirectorReport {
            entries: vec![
                DirectorEntry {
                    role: AgentRole::Researcher,
                    verified: true,
                    verdict: None,
                    result: ok.clone(),
                },
                DirectorEntry {
                    role: AgentRole::Verifier,
                    verified: false,
                    verdict: Some("child ended failed".into()),
                    result: AgentResult::failed("verifier.2", "no", vec![], "boom"),
                },
            ],
            total_duration_ms: 4,
        };
        assert!(!with_failure.all_verified());
        assert_eq!(with_failure.verified(), 1);
        assert_eq!(with_failure.summary_lines().len(), 2);
    }

    #[test]
    fn empty_report_is_never_all_verified() {
        let report = DirectorReport {
            entries: vec![],
            total_duration_ms: 0,
        };
        assert!(!report.all_verified());
    }

    #[test]
    fn verified_observation_is_exposed_only_when_every_child_is_verified() {
        let with_obs = ok_result("researcher.1").with_observation(json!({ "stdout": "a" }));

        let all_verified = DirectorReport {
            entries: vec![DirectorEntry {
                role: AgentRole::Researcher,
                verified: true,
                verdict: None,
                result: with_obs.clone(),
            }],
            total_duration_ms: 1,
        };
        assert_eq!(
            all_verified.verified_observation().unwrap()["stdout"],
            json!("a")
        );

        // A verified child without a payload has nothing to hand up.
        let no_payload = DirectorReport {
            entries: vec![DirectorEntry {
                role: AgentRole::Researcher,
                verified: true,
                verdict: None,
                result: ok_result("researcher.2"),
            }],
            total_duration_ms: 1,
        };
        assert!(no_payload.verified_observation().is_none());

        // Any unverified child refuses the whole observation, even if the
        // first child carried one.
        let mixed = DirectorReport {
            entries: vec![
                DirectorEntry {
                    role: AgentRole::Researcher,
                    verified: true,
                    verdict: None,
                    result: with_obs,
                },
                DirectorEntry {
                    role: AgentRole::Verifier,
                    verified: false,
                    verdict: Some("child ended failed".into()),
                    result: ok_result("verifier.2"),
                },
            ],
            total_duration_ms: 2,
        };
        assert!(mixed.verified_observation().is_none());
    }
}
