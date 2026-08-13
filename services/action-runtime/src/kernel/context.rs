//! Execution context: the immutable envelope every action executes inside.
//!
//! A context binds an action to its session, working directory, environment,
//! permissions, trace, and — crucially — a [`CancellationToken`] for
//! propagation. Contexts are immutable after creation; the only mutable handles
//! are the cancellation token (shared, interior-mutable) and progress, which
//! lives outside the context. Nested plans derive child contexts that inherit
//! the parent's session/identity but are isolated for their own variables; a
//! later [`ExecutionContext::merge`] folds parallel branches back together.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use friday_agent_runtime::SecurityScope;
use serde_json::Value;
use tokio::sync::Notify;
use uuid::Uuid;

/// A cancellation signal that propagates to children.
///
/// Cloning yields a token observing the same underlying state, so a parent and
/// all its descendants share one cancellation decision. Cancelling the parent
/// cancels every child automatically.
#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

struct TokenInner {
    cancelled: AtomicBool,
    notify: Notify,
    children: std::sync::Mutex<Vec<CancellationToken>>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                children: std::sync::Mutex::new(Vec::new()),
            }),
        }
    }

    /// Derive a child token that is cancelled together with (and after) this one.
    pub fn child(&self) -> CancellationToken {
        let child = CancellationToken::new();
        self.inner
            .children
            .lock()
            .expect("token children poisoned")
            .push(child.clone());
        child
    }

    /// Cancel this token and every descendant.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
        for child in self
            .inner
            .children
            .lock()
            .expect("token children poisoned")
            .iter()
        {
            child.cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve as soon as this token (or an ancestor) is cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

/// The immutable envelope carried by every action.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub execution_id: Uuid,
    pub session_id: Uuid,
    pub parent_execution: Option<Uuid>,
    pub user_id: String,
    pub cwd: PathBuf,
    pub environment: HashMap<String, String>,
    pub variables: HashMap<String, Value>,
    pub permissions: Vec<String>,
    pub trace_id: Uuid,
    pub deadline: Option<DateTime<Utc>>,
    pub cancellation_token: CancellationToken,
    pub metadata: HashMap<String, Value>,
    /// Security authority attached to this execution. `None` means no boundary
    /// has been attached yet (the egress policy is not enforced for the
    /// request); `Some` grants/denies egress destinations at the kernel
    /// boundary. Children inherit the parent's scope.
    pub security_scope: Option<SecurityScope>,
}

impl ExecutionContext {
    /// Begin building a context with sensible defaults.
    pub fn builder() -> ExecutionContextBuilder {
        ExecutionContextBuilder::default()
    }

    /// Derive an isolated child context that inherits this one's session and
    /// identity but is cancelled together with it. The child's
    /// `parent_execution` points back at this context.
    pub fn child(&self, execution_id: Uuid) -> ExecutionContext {
        ExecutionContext {
            execution_id,
            session_id: self.session_id,
            parent_execution: Some(self.execution_id),
            user_id: self.user_id.clone(),
            cwd: self.cwd.clone(),
            environment: self.environment.clone(),
            variables: self.variables.clone(),
            permissions: self.permissions.clone(),
            trace_id: self.trace_id,
            deadline: self.deadline,
            cancellation_token: self.cancellation_token.child(),
            metadata: self.metadata.clone(),
            security_scope: self.security_scope.clone(),
        }
    }

    /// Fold parallel child branches back into a single continuation context.
    /// Child variable/metadata entries override the parent's; identity (session,
    /// trace, permissions) is inherited from `parent`.
    pub fn merge(parent: &ExecutionContext, children: &[ExecutionContext]) -> ExecutionContext {
        let mut variables = parent.variables.clone();
        let mut metadata = parent.metadata.clone();
        for child in children {
            for (k, v) in &child.variables {
                variables.insert(k.clone(), v.clone());
            }
            for (k, v) in &child.metadata {
                metadata.insert(k.clone(), v.clone());
            }
        }
        ExecutionContext::builder()
            .execution_id(Uuid::new_v4())
            .session_id(parent.session_id)
            .parent_execution(parent.parent_execution)
            .user_id(parent.user_id.clone())
            .cwd(parent.cwd.clone())
            .environment(parent.environment.clone())
            .variables(variables)
            .permissions(parent.permissions.clone())
            .trace_id(parent.trace_id)
            .metadata(metadata)
            .security_scope(parent.security_scope.clone())
            .build()
    }

    /// True if a deadline is set and already passed.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self.deadline {
            Some(d) => now > d,
            None => false,
        }
    }

    /// Remaining time until the deadline, if any.
    pub fn remaining(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.deadline.map(|d| {
            let secs = (d - now).num_milliseconds();
            Duration::from_millis(secs.max(0) as u64)
        })
    }
}

/// Ergonomic builder for [`ExecutionContext`]. All fields have safe defaults;
/// only `execution_id` and `session_id` are required (generated if not set).
pub struct ExecutionContextBuilder {
    execution_id: Uuid,
    session_id: Uuid,
    parent_execution: Option<Uuid>,
    user_id: String,
    cwd: PathBuf,
    environment: HashMap<String, String>,
    variables: HashMap<String, Value>,
    permissions: Vec<String>,
    trace_id: Uuid,
    deadline: Option<DateTime<Utc>>,
    metadata: HashMap<String, Value>,
    security_scope: Option<SecurityScope>,
}

impl Default for ExecutionContextBuilder {
    fn default() -> Self {
        Self {
            execution_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            parent_execution: None,
            user_id: "user".to_string(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            environment: std::env::vars().collect(),
            variables: HashMap::new(),
            permissions: Vec::new(),
            trace_id: Uuid::new_v4(),
            deadline: None,
            metadata: HashMap::new(),
            security_scope: None,
        }
    }
}

impl ExecutionContextBuilder {
    pub fn execution_id(mut self, id: Uuid) -> Self {
        self.execution_id = id;
        self
    }
    pub fn session_id(mut self, id: Uuid) -> Self {
        self.session_id = id;
        self
    }
    pub fn parent_execution(mut self, id: Option<Uuid>) -> Self {
        self.parent_execution = id;
        self
    }
    pub fn user_id(mut self, u: impl Into<String>) -> Self {
        self.user_id = u.into();
        self
    }
    pub fn cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = cwd;
        self
    }
    pub fn environment(mut self, env: HashMap<String, String>) -> Self {
        self.environment = env;
        self
    }
    pub fn variable(mut self, k: impl Into<String>, v: Value) -> Self {
        self.variables.insert(k.into(), v);
        self
    }
    pub fn variables(mut self, vars: HashMap<String, Value>) -> Self {
        self.variables = vars;
        self
    }
    pub fn permissions(mut self, p: Vec<String>) -> Self {
        self.permissions = p;
        self
    }
    pub fn trace_id(mut self, t: Uuid) -> Self {
        self.trace_id = t;
        self
    }
    pub fn deadline(mut self, d: DateTime<Utc>) -> Self {
        self.deadline = Some(d);
        self
    }
    pub fn metadata(mut self, m: HashMap<String, Value>) -> Self {
        self.metadata = m;
        self
    }
    pub fn security_scope(mut self, scope: Option<SecurityScope>) -> Self {
        self.security_scope = scope;
        self
    }
    pub fn build(self) -> ExecutionContext {
        ExecutionContext {
            execution_id: self.execution_id,
            session_id: self.session_id,
            parent_execution: self.parent_execution,
            user_id: self.user_id,
            cwd: self.cwd,
            environment: self.environment,
            variables: self.variables,
            permissions: self.permissions,
            trace_id: self.trace_id,
            deadline: self.deadline,
            cancellation_token: CancellationToken::new(),
            metadata: self.metadata,
            security_scope: self.security_scope,
        }
    }
}
