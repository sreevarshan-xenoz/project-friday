//! The Action Runtime (M4.1 + M4.4.5): the decoupled dispatcher every
//! capability flows through, now sitting on the stable kernel.
//!
//! It subscribes to `exec.action` from the planner's Execution Engine. For each
//! one it:
//!
//! 1. builds an immutable [`ExecutionContext`] (bound to a session that
//!    continues across the plan's actions),
//! 2. asks the [`PermissionEngine`] whether the action may proceed,
//! 3. dispatches to the matching [`ActionAdapter`] with a timeout, deadline and
//!    **cancellation** (via the context token) plus bounded retries,
//! 4. records an [`AuditEntry`] and a [`MetricSample`],
//! 5. drives the [`LifecycleManager`] for the execution,
//! 6. emits a normalized `exec.result` (carrying `execution_id`) back onto the bus.
//!
//! The runtime knows nothing about terminals or files — it only routes,
//! enforces permissions, retries, measures, audits, and owns lifecycle. See
//! `docs/Execution.md`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use friday_core::event::Event;
use friday_core::plugin::{Plugin, PluginMetadata};
use friday_planner::events;
use serde_json::json;
use uuid::Uuid;

use crate::adapters::ActionAdapter;
use crate::audit::{AuditEngine, AuditEntry};
use crate::kernel::{ExecutionContext, Kernel, LifecycleState, MetricSample};
use crate::permission::{PermissionDecision, PermissionEngine};
use crate::request::{ActionRequest, ActionResult};

/// Default ceiling on how long a single action attempt may run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default number of retry attempts after the first failure.
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// The decoupled action dispatcher. Registered as a plugin; owns the adapter
/// registry, the permission engine, the audit engine, and the kernel (lifecycle,
/// metrics, sessions, resources).
pub struct AgentKernel {
    meta: PluginMetadata,
    adapters: tokio::sync::Mutex<HashMap<String, Box<dyn ActionAdapter>>>,
    permission: PermissionEngine,
    audit: AuditEngine,
    kernel: Kernel,
    default_timeout: Duration,
    max_retries: u32,
    /// Maps a planner `plan_id` to a stable session so all of its actions share
    /// one continuous context.
    sessions_by_plan: StdMutex<HashMap<String, Uuid>>,
}

impl Default for AgentKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentKernel {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("action-runtime", "Action Runtime")
            .description("Decoupled dispatcher: context, permission, dispatch, retry, audit")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .dependencies(&["execution"])
            .build();
        Self {
            meta,
            adapters: tokio::sync::Mutex::new(HashMap::new()),
            permission: PermissionEngine::new(),
            audit: AuditEngine::new(1024),
            kernel: Kernel::new(),
            default_timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
            sessions_by_plan: StdMutex::new(HashMap::new()),
        }
    }

    /// Override the permission engine. Lets deployments inject policy (e.g.
    /// deny specific tool/action pairs) without forking the runtime. The engine
    /// applies uniformly to every backend — native, plugin, and MCP — so no
    /// backend needs its own permission logic.
    pub fn with_permission(mut self, permission: PermissionEngine) -> Self {
        self.permission = permission;
        self
    }

    /// Override the default per-attempt timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Override the retry budget.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Register a capability adapter under its `tool()` name.
    pub async fn register_adapter(&self, adapter: Box<dyn ActionAdapter>) {
        let tool = adapter.tool().to_string();
        tracing::info!(tool = %tool, "registering action adapter");
        self.adapters.lock().await.insert(tool, adapter);
    }

    /// Register a live MCP server as a first-class capability adapter (M8:
    /// Execution Runtime Unification). The transport is connected, the server is
    /// initialized, its tools are listed, and a [`crate::adapters::McpAdapter`]
    /// is registered under the tool key `mcp:<server>` — identical to a native
    /// adapter. After this call, an `exec.action` for `mcp:<server>.<tool>` is
    /// dispatched through the unified `run_action` path with no MCP-specific
    /// branching anywhere in the supervisor/planner/kernel.
    ///
    /// Returns the discovered tool names so the caller can also register them in
    /// the planner's [`friday_planner::execution::capability_registry::CapabilityRegistry`].
    pub async fn register_mcp_server(
        &self,
        server: &str,
        transport: Box<dyn friday_mcp::transport::Transport>,
    ) -> Result<Vec<friday_mcp::protocol::McpTool>, String> {
        let mut client = friday_mcp::protocol::McpClient::new(transport);
        client
            .initialize()
            .await
            .map_err(|e| format!("mcp initialize failed for {server}: {e}"))?;
        let tools = client
            .list_tools()
            .await
            .map_err(|e| format!("mcp list_tools failed for {server}: {e}"))?;
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        let adapter: Box<dyn ActionAdapter> = Box::new(crate::adapters::McpAdapter::new(
            server,
            client,
            tool_names.clone(),
        ));
        self.register_adapter(adapter).await;
        Ok(tools)
    }

    /// Access the audit engine (for tests/inspection).
    pub fn audit(&self) -> &AuditEngine {
        &self.audit
    }

    /// Access the kernel (lifecycle, metrics, sessions, resources).
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    /// Split a planner `capability` (e.g. `filesystem.create_file`) into
    /// `(tool, action)`.
    fn split_capability(capability: &str) -> (String, String) {
        match capability.split_once('.') {
            Some((tool, action)) => (tool.to_string(), action.to_string()),
            None => (capability.to_string(), String::new()),
        }
    }

    /// Return (creating if needed) the session that a plan's actions share.
    fn session_for(&self, plan_id: &str) -> Uuid {
        if let Some(id) = self.sessions_by_plan.lock().unwrap().get(plan_id) {
            return *id;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let id = self.kernel.sessions.create("plan", cwd);
        self.sessions_by_plan
            .lock()
            .unwrap()
            .insert(plan_id.to_string(), id);
        id
    }

    async fn on_exec_action(&self, event: &Event) -> Vec<Event> {
        let task_id = event
            .payload
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_id = event
            .payload
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let capability = event
            .payload
            .get("capability")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let params = event
            .payload
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let (tool, action) = Self::split_capability(&capability);
        let req = ActionRequest::new(tool, action, params);

        let execution_id = Uuid::parse_str(&task_id).unwrap_or_else(|_| Uuid::new_v4());
        let session_id = self.session_for(&plan_id);
        let session = self.kernel.sessions.get(session_id);
        let cwd = session
            .as_ref()
            .map(|s| s.cwd.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let environment = session
            .as_ref()
            .map(|s| s.environment.clone())
            .unwrap_or_default();

        let ctx = ExecutionContext::builder()
            .execution_id(execution_id)
            .session_id(session_id)
            .trace_id(event.trace_id)
            .cwd(cwd)
            .environment(environment)
            .build();

        let result = self.run_action(req, ctx).await;
        let mut events = vec![Self::result_event(
            &task_id,
            &plan_id,
            &capability,
            &result,
            event.trace_id,
            execution_id,
        )];
        // M9.3: an adapter may annotate created artifacts; emit one
        // `artifact.created` per path so reflection/memory can track real output.
        if let Some(paths) = result.metadata.get("artifact_paths").and_then(|p| p.as_array()) {
            for path in paths {
                if let Some(p) = path.as_str() {
                    events.push(events::event(
                        events::ARTIFACT_CREATED,
                        "action-runtime",
                        json!({
                            "path": p,
                            "capability": capability,
                            "task_id": task_id,
                            "plan_id": plan_id,
                        }),
                        event.trace_id,
                    ));
                }
            }
        }
        // M10.2: Git-specific domain events. The adapter annotates metadata;
        // the runtime owns event emission (adapters never publish events).
        if result.metadata.contains_key("commit_hash") {
            let mut payload = json!({
                "commit_hash": result.metadata.get("commit_hash"),
                "capability": capability,
                "task_id": task_id,
                "plan_id": plan_id,
            });
            if let Some(branch) = result.metadata.get("branch") {
                payload["branch"] = branch.clone();
            }
            if let Some(files) = result.metadata.get("files_changed") {
                payload["files_changed"] = files.clone();
            }
            events.push(events::event(
                events::GIT_COMMIT_CREATED,
                "action-runtime",
                payload,
                event.trace_id,
            ));
        }
        if let Some(branch) = result.metadata.get("branch").and_then(|b| b.as_str()) {
            // Only emit for branch-changing actions, not every git_branch call.
            if capability.contains("git_checkout") || capability.contains("git_create_branch") {
                events.push(events::event(
                    events::GIT_BRANCH_CHANGED,
                    "action-runtime",
                    json!({
                        "branch": branch,
                        "capability": capability,
                        "task_id": task_id,
                        "plan_id": plan_id,
                    }),
                    event.trace_id,
                ));
            }
        }
        if let Some(files) = result.metadata.get("files_affected").and_then(|f| f.as_u64()) {
            events.push(events::event(
                events::GIT_DIFF_GENERATED,
                "action-runtime",
                json!({
                    "files_affected": files,
                    "capability": capability,
                    "task_id": task_id,
                    "plan_id": plan_id,
                }),
                event.trace_id,
            ));
        }
        events
    }

    /// Execute a normalized request inside a context. This is the single, public
    /// dispatch path — usable both from the event bus and directly (tests,
    /// future schedulers). It enforces permission, timeout, deadline,
    /// cancellation, retries, metrics, audit, and lifecycle.
    pub async fn run_action(&self, req: ActionRequest, ctx: ExecutionContext) -> ActionResult {
        let execution_id = ctx.execution_id;
        let trace = ctx.trace_id;
        let decision = self.permission.decide(&req);
        let decision_str = format!("{decision:?}");

        self.kernel.lifecycle.create(execution_id);
        self.kernel
            .lifecycle
            .transition(execution_id, LifecycleState::Running)
            .ok();

        if decision == PermissionDecision::Deny {
            self.audit.record(self.make_audit(
                &req,
                &decision_str,
                trace,
                0,
                false,
                Some("denied by permission engine".into()),
            ));
            self.kernel
                .lifecycle
                .transition(execution_id, LifecycleState::Cancelled)
                .ok();
            return ActionResult::error("permission denied");
        }

        let start = std::time::Instant::now();
        let adapters = self.adapters.lock().await;
        let adapter = match adapters.get(&req.tool) {
            Some(a) => a,
            None => {
                let latency = start.elapsed().as_millis();
                self.audit.record(self.make_audit(
                    &req,
                    &decision_str,
                    trace,
                    latency,
                    false,
                    Some(format!("no adapter registered for tool '{}'", req.tool)),
                ));
                self.kernel
                    .lifecycle
                    .transition(execution_id, LifecycleState::Cancelled)
                    .ok();
                self.record_metrics(execution_id, &req.tool, latency, 0, false);
                return ActionResult::error(format!(
                    "no adapter registered for tool '{}'",
                    req.tool
                ));
            }
        };

        let mut last = ActionResult::error("not executed");
        let mut retries = 0u32;
        for attempt in 0..=self.max_retries {
            let now = Utc::now();
            let mut timeout = req.timeout.unwrap_or(self.default_timeout);
            if let Some(rem) = ctx.remaining(now) {
                timeout = timeout.min(rem);
            }
            tokio::select! {
                _ = ctx.cancellation_token.cancelled() => {
                    last = ActionResult::error("cancelled");
                    break;
                }
                res = tokio::time::timeout(timeout, adapter.execute(req.clone())) => {
                    match res {
                        Ok(r) => {
                            last = r;
                            if last.success {
                                break;
                            }
                        }
                        Err(_) => {
                            last = ActionResult::error("action timed out");
                        }
                    }
                }
            }
            if !last.success && attempt < self.max_retries {
                retries += 1;
                tracing::warn!(
                    tool = %req.tool,
                    action = %req.action,
                    attempt,
                    "action failed; retrying"
                );
            }
        }

        let latency = start.elapsed().as_millis();
        let cancelled = last.error.as_deref() == Some("cancelled");
        let final_state = if cancelled {
            LifecycleState::Cancelled
        } else {
            LifecycleState::Completed
        };
        self.kernel
            .lifecycle
            .transition(execution_id, final_state)
            .ok();
        self.audit.record(self.make_audit(
            &req,
            &decision_str,
            trace,
            latency,
            last.success,
            last.error.clone(),
        ));
        self.record_metrics(execution_id, &req.tool, latency, retries, last.success);
        last.with_latency_ms(latency)
    }

    fn record_metrics(
        &self,
        execution_id: Uuid,
        plugin: &str,
        execution_ms: u128,
        retry_count: u32,
        success: bool,
    ) {
        self.kernel.metrics.record(MetricSample {
            execution_id,
            plugin_used: plugin.to_string(),
            execution_ms,
            retry_count,
            success,
            ..Default::default()
        });
    }

    fn make_audit(
        &self,
        req: &ActionRequest,
        decision: &str,
        trace: Uuid,
        latency_ms: u128,
        success: bool,
        detail: Option<String>,
    ) -> AuditEntry {
        AuditEntry {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            trace_id: trace,
            tool: req.tool.clone(),
            action: req.action.clone(),
            decision: decision.to_string(),
            latency_ms,
            success,
            detail,
        }
    }

    fn result_event(
        task_id: &str,
        plan_id: &str,
        capability: &str,
        result: &ActionResult,
        trace: Uuid,
        execution_id: Uuid,
    ) -> Event {
        events::event(
            events::EXEC_RESULT,
            "action-runtime",
            json!({
                "task_id": task_id,
                "plan_id": plan_id,
                "capability": capability,
                "execution_id": execution_id.to_string(),
                "success": result.success,
                "output": result.output,
                "error": result.error,
            }),
            trace,
        )
    }
}

#[async_trait]
impl Plugin for AgentKernel {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> {
        Ok(match event.kind.as_str() {
            events::EXEC_ACTION => self.on_exec_action(event).await,
            _ => vec![],
        })
    }
}
