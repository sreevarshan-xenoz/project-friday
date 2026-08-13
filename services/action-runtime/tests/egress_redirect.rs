//! #12-C2 redirect-enforcement acceptance tests.
//!
//! The kernel's pre-dispatch gate (#12-C1) only sees the *first* URL. This
//! suite proves the second seam: every redirect hop is classified against the
//! same egress policy **before** the next request is sent, using a local
//! deterministic HTTP server (never the internet):
//!
//! ```text
//! /public
//!    ↓ 302 → http://127.0.0.1:<port>/private
//!    ↓ (egress deny)  ✋ NO SECOND REQUEST
//! ```
//!
//! Proven here:
//!
//! - an internal redirect target (loopback) is denied with the adapter having
//!   made exactly one request to `/public` and zero to `/private`;
//! - the denied target (and any secret embedded in it) never leaks into the
//!   error surface or the audit trail;
//! - an out-of-scope external redirect is denied before the target is
//!   contacted;
//! - a relative `Location` is resolved and still checked;
//! - an in-scope redirect passes the guard (the follow itself then fails
//!   deterministically on the reserved `.example` TLD, never the network);
//! - the kernel audits the denial as `EgressRedirectDeny`, cancels the
//!   execution, and does not retry.

use async_trait::async_trait;
use friday_actions::kernel::{ExecutionContext, LifecycleState};
use friday_actions::request::{ActionRequest, ActionResult};
use friday_actions::runtime::AgentKernel;
use friday_actions::ActionAdapter;
use friday_actions::WebAdapter;
use friday_agent_runtime::SecurityScope;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// One canned response for a mock-server route.
#[derive(Clone)]
struct MockResponse {
    status: u16,
    location: Option<String>,
    body: String,
}

impl MockResponse {
    fn redirect(location: impl Into<String>) -> Self {
        Self {
            status: 302,
            location: Some(location.into()),
            body: String::new(),
        }
    }
}

/// A minimal HTTP/1.1 server on 127.0.0.1 that counts hits per path. One
/// request per connection (`Connection: close`), so every adapter hop is a
/// fresh, individually-counted request. Routes are built *after* binding so a
/// `Location` can embed the exact loopback address the server listens on.
struct MockServer {
    addr: std::net::SocketAddr,
    counts: Arc<Mutex<HashMap<String, usize>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockServer {
    async fn start(routes: impl FnOnce(u16) -> Vec<(&'static str, MockResponse)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server addr");
        let routes: HashMap<String, MockResponse> = routes(addr.port())
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let counts = Arc::new(Mutex::new(HashMap::new()));
        let counts_task = Arc::clone(&counts);
        let handle = tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let routes = routes.clone();
                let counts = Arc::clone(&counts_task);
                tokio::spawn(async move {
                    let _ = serve(&mut sock, &routes, &counts).await;
                });
            }
        });
        Self {
            addr,
            counts,
            handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.addr.port(), path)
    }

    fn hits(&self, path: &str) -> usize {
        *self.counts.lock().unwrap().get(path).unwrap_or(&0)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve(
    sock: &mut TcpStream,
    routes: &HashMap<String, MockResponse>,
    counts: &Arc<Mutex<HashMap<String, usize>>>,
) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let mut read = 0;
    loop {
        let n = sock.read(&mut buf[read..]).await?;
        if n == 0 {
            break;
        }
        read += n;
        if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") || read == buf.len() {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf[..read]);
    let path = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();
    {
        let mut c = counts.lock().unwrap();
        *c.entry(path.clone()).or_insert(0) += 1;
    }
    let resp = routes.get(&path).cloned().unwrap_or(MockResponse {
        status: 404,
        location: None,
        body: "not found".into(),
    });
    let status_text = match resp.status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        _ => "Status",
    };
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        status_text,
        resp.body.len()
    );
    if let Some(loc) = &resp.location {
        out.push_str(&format!("Location: {loc}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    sock.write_all(out.as_bytes()).await
}

fn fetch(url: &str) -> ActionRequest {
    ActionRequest::new("web", "fetch", json!({ "url": url }))
}

fn ctx(scope: Option<SecurityScope>) -> ExecutionContext {
    ExecutionContext::builder()
        .execution_id(uuid::Uuid::new_v4())
        .session_id(uuid::Uuid::new_v4())
        .security_scope(scope)
        .build()
}

#[tokio::test]
async fn internal_redirect_is_refused_before_the_second_request() {
    let secret = "sk-redirect-secret-999";
    let server = MockServer::start(|port| {
        vec![
            (
                "/public",
                MockResponse::redirect(format!("http://127.0.0.1:{port}/private?token={secret}")),
            ),
            (
                "/private",
                MockResponse {
                    status: 200,
                    location: None,
                    body: "PRIVATE".into(),
                },
            ),
        ]
    })
    .await;

    // Unscoped: internal destinations are denied unconditionally.
    let adapter = WebAdapter::new();
    let res = adapter.execute(fetch(&server.url("/public"))).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(
        res.metadata.get("egress_deny").and_then(|v| v.as_str()),
        Some("internal_destination"),
        "the reason is a static classifier label"
    );
    assert_eq!(
        server.hits("/public"),
        1,
        "the initial request happened once"
    );
    assert_eq!(
        server.hits("/private"),
        0,
        "the redirected request must never be sent"
    );

    let err = res.error.unwrap_or_default();
    assert!(!err.contains(secret), "error leaked the secret: {err}");

    // Scoped: even a scope that grants the literal cannot open internal nets.
    let adapter = WebAdapter::new();
    let mut req = fetch(&server.url("/public"));
    req.security_scope = Some(SecurityScope::new(&["127.0.0.1", "example.com"], &[]));
    let res = adapter.execute(req).await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(server.hits("/public"), 2, "two separate requests total");
    assert_eq!(server.hits("/private"), 0);
}

#[tokio::test]
async fn out_of_scope_redirect_is_refused_before_contacting_the_target() {
    let server = MockServer::start(|_port| {
        vec![(
            "/public",
            MockResponse::redirect("https://notcovered.example/private"),
        )]
    })
    .await;

    let adapter = WebAdapter::new();
    let mut req = fetch(&server.url("/public"));
    req.security_scope = Some(SecurityScope::new(&["example.com"], &[]));
    let res = adapter.execute(req).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(
        res.metadata.get("egress_deny").and_then(|v| v.as_str()),
        Some("destination_not_covered_by_scope")
    );
    assert_eq!(server.hits("/public"), 1);
}

#[tokio::test]
async fn relative_redirect_location_is_resolved_and_checked() {
    let server = MockServer::start(|_port| {
        vec![
            ("/public", MockResponse::redirect("/private")),
            (
                "/private",
                MockResponse {
                    status: 200,
                    location: None,
                    body: "PRIVATE".into(),
                },
            ),
        ]
    })
    .await;

    let adapter = WebAdapter::new();
    let res = adapter.execute(fetch(&server.url("/public"))).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(
        res.metadata.get("egress_deny").and_then(|v| v.as_str()),
        Some("internal_destination")
    );
    assert_eq!(server.hits("/public"), 1);
    assert_eq!(server.hits("/private"), 0);
}

#[tokio::test]
async fn in_scope_redirect_passes_the_guard() {
    let server = MockServer::start(|_port| {
        vec![(
            "/public",
            MockResponse::redirect("https://allowed.example/private"),
        )]
    })
    .await;

    let adapter = WebAdapter::new();
    let mut req = fetch(&server.url("/public"));
    req.security_scope = Some(SecurityScope::new(&["allowed.example"], &[]));
    let res = adapter.execute(req).await;

    // The guard allows the hop (it is covered by the scope), so the follow is
    // attempted — and fails deterministically on the reserved `.example` TLD,
    // which never resolves. The point is that this is a *network* failure, not
    // an egress denial.
    assert!(!res.success);
    assert!(
        !res.error.as_deref().unwrap_or("").contains("egress"),
        "an in-scope redirect must not be treated as an egress denial: {:?}",
        res.error
    );
    assert_eq!(server.hits("/public"), 1);
}

/// A `web` adapter that simulates what `WebAdapter` returns when it refuses a
/// redirect hop, so the kernel's audit wiring can be tested hermetically
/// without a real network round-trip.
struct RedirectDenyingAdapter {
    calls: Arc<AtomicUsize>,
    label: &'static str,
}

#[async_trait]
impl ActionAdapter for RedirectDenyingAdapter {
    fn tool(&self) -> &str {
        "web"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["fetch"]
    }

    async fn execute(&self, _req: ActionRequest) -> ActionResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ActionResult::error("egress denied").with_metadata("egress_deny", json!(self.label))
    }
}

#[tokio::test]
async fn kernel_audits_redirect_denial_cancels_and_does_not_retry() {
    let secret = "sk-kernel-secret-4242";
    let calls = Arc::new(AtomicUsize::new(0));
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(RedirectDenyingAdapter {
            calls: Arc::clone(&calls),
            label: "internal_destination",
        }))
        .await;

    let scope = SecurityScope::new(&["example.com"], &[]);
    let c = ctx(Some(scope));
    let execution_id = c.execution_id;
    let req = ActionRequest::new(
        "web",
        "fetch",
        json!({ "url": format!("https://example.com/ok?token={secret}") }),
    );
    let res = kernel.run_action(req, c).await;

    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("egress denied"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "denial is terminal, no retry"
    );
    assert_eq!(
        kernel.kernel().lifecycle.state(execution_id),
        Some(LifecycleState::Cancelled),
        "an egress redirect denial cancels the execution"
    );

    let entries = kernel.audit().recent(10);
    let denied = entries
        .iter()
        .find(|e| e.decision == "EgressRedirectDeny")
        .expect("an EgressRedirectDeny audit entry must exist");
    let detail = denied.detail.clone().unwrap_or_default();
    assert_eq!(detail, "denied by egress policy (internal_destination)");
    assert!(
        !detail.contains(secret),
        "audit leaked the secret: {detail}"
    );
    assert!(
        !detail.contains("example.com"),
        "audit leaked the destination: {detail}"
    );
}

#[tokio::test]
async fn kernel_ignores_unrecognized_deny_labels() {
    let calls = Arc::new(AtomicUsize::new(0));
    let kernel = Arc::new(AgentKernel::new().with_timeout(Duration::from_secs(5)));
    kernel
        .register_adapter(Box::new(RedirectDenyingAdapter {
            calls: Arc::clone(&calls),
            label: "https://smuggled.example/secret",
        }))
        .await;

    let scope = SecurityScope::new(&["example.com"], &[]);
    let c = ctx(Some(scope));
    let execution_id = c.execution_id;
    let req = ActionRequest::new("web", "fetch", json!({ "url": "https://example.com/ok" }));
    let res = kernel.run_action(req, c).await;

    // A non-canonical label is not an egress signal: the failure is just a
    // failed action (still terminal after max retries), and nothing lands in
    // an EgressRedirectDeny audit entry.
    assert!(!res.success);
    assert_eq!(
        kernel.kernel().lifecycle.state(execution_id),
        Some(LifecycleState::Completed)
    );
    let entries = kernel.audit().recent(10);
    assert!(
        !entries.iter().any(|e| e.decision == "EgressRedirectDeny"),
        "unrecognized labels must not be audited as egress denials"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3, "default retries apply");
}
