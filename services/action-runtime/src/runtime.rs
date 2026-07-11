//! The Action Runtime (M4.1): the decoupled dispatcher every capability flows
//! through.
//!
//! It subscribes to `exec.action` from the planner's Execution Engine. For each
//! one it:
//!
//! 1. translates the planner's `capability` into a normalized [`ActionRequest`],
//! 2. asks the [`PermissionEngine`] whether the action may proceed,
//! 3. dispatches to the matching [`ActionAdapter`] with a timeout and bounded
//!    retries,
//! 4. records an [`AuditEntry`],
//! 5. emits a normalized `exec.result` back onto the bus.
//!
//! The runtime knows nothing about terminals or files — it only routes,
//! enforces permissions, retries, measures, and audits. See `docs/Execution.md`.

use std::collections::HashMap;
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
use crate::permission::{PermissionDecision, PermissionEngine};
use crate::request::{ActionRequest, ActionResult};

/// Default ceiling on how long a single action attempt may run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default number of retry attempts after the first failure.
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// The decoupled action dispatcher. Registered as a plugin; owns the adapter
/// registry, the permission engine, and the audit engine.
pub struct ActionRuntime {
    meta: PluginMetadata,
    adapters: tokio::sync::Mutex<HashMap<String, Box<dyn ActionAdapter>>>,
    permission: PermissionEngine,
    audit: AuditEngine,
    default_timeout: Duration,
    max_retries: u32,
}

impl Default for ActionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRuntime {
    pub fn new() -> Self {
        let meta = PluginMetadata::builder("action-runtime", "Action Runtime")
            .description("Decoupled dispatcher: permission, dispatch, retry, audit")
            .subscribes(&[events::EXEC_ACTION])
            .emits(&[events::EXEC_RESULT])
            .dependencies(&["execution"])
            .build();
        Self {
            meta,
            adapters: tokio::sync::Mutex::new(HashMap::new()),
            permission: PermissionEngine::new(),
            audit: AuditEngine::new(1024),
            default_timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
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

    /// Access the audit engine (for tests/inspection).
    pub fn audit(&self) -> &AuditEngine {
        &self.audit
    }

    /// Split a planner `capability` (e.g. `filesystem.create_file`) into
    /// `(tool, action)`.
    fn split_capability(capability: &str) -> (String, String) {
        match capability.split_once('.') {
            Some((tool, action)) => (tool.to_string(), action.to_string()),
            None => (capability.to_string(), String::new()),
        }
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
        let result = self.dispatch(req, event.trace_id).await;
        vec![Self::result_event(
            &task_id,
            &plan_id,
            &result,
            event.trace_id,
        )]
    }

    async fn dispatch(&self, req: ActionRequest, trace: Uuid) -> ActionResult {
        let decision = self.permission.decide(&req);
        let decision_str = format!("{decision:?}");

        if decision == PermissionDecision::Deny {
            self.audit.record(AuditEntry {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                trace_id: trace,
                tool: req.tool.clone(),
                action: req.action.clone(),
                decision: decision_str,
                latency_ms: 0,
                success: false,
                detail: Some("denied by permission engine".to_string()),
            });
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
                return ActionResult::error(format!(
                    "no adapter registered for tool '{}'",
                    req.tool
                ));
            }
        };

        let mut last = ActionResult::error("not executed");
        for attempt in 0..=self.max_retries {
            let timeout = req.timeout.unwrap_or(self.default_timeout);
            match tokio::time::timeout(timeout, adapter.execute(req.clone())).await {
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
            if attempt < self.max_retries && !last.success {
                tracing::warn!(
                    tool = %req.tool,
                    action = %req.action,
                    attempt,
                    "action failed; retrying"
                );
            }
        }

        let latency = start.elapsed().as_millis();
        self.audit.record(self.make_audit(
            &req,
            &decision_str,
            trace,
            latency,
            last.success,
            last.error.clone(),
        ));
        last.with_latency_ms(latency)
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

    fn result_event(task_id: &str, plan_id: &str, result: &ActionResult, trace: Uuid) -> Event {
        events::event(
            events::EXEC_RESULT,
            "action-runtime",
            json!({
                "task_id": task_id,
                "plan_id": plan_id,
                "success": result.success,
                "output": result.output,
                "error": result.error,
            }),
            trace,
        )
    }
}

#[async_trait]
impl Plugin for ActionRuntime {
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
