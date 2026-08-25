//! Real browser capability adapter (M7): drives a headless Chrome/Chromium via
//! CDP (chromiumoxide) to serve the `web.open / click / type / scroll /
//! extract / screenshot` verbs the `web` skill soft-declares.
//!
//! The `web` skill declares these verbs as **soft-declared** capabilities —
//! they flip to `Resolved` the moment a host calls
//! [`register_browser_capabilities`] and `reconcile`s the skill registry. That
//! is the designed "browser runtime arrives" moment.
//!
//! **Session model:** the adapter keeps one browser alive across actions (the
//! last opened page stays current). A per-adapter `user_data_dir` persists
//! cookies/localStorage on disk so sessions survive adapter restarts; a
//! `connect_url` (Chrome launched with `--remote-debugging-port`) can attach to
//! an externally-managed browser instead of launching one.
//!
//! **Trust note:** `url` is scheme-guarded (http/https only) and navigation is
//! driven by host-supplied params, the same trust class as the HTTP
//! [`crate::adapters::WebAdapter`] — the permission layer is the boundary.
//! `click`/`type`/`submit` are classified `Risk::Medium` (mutating a live page);
//! `open`/`extract`/`scroll`/`screenshot` are `Risk::Safe`; `evaluate`
//! (arbitrary page JS) is `Risk::High` and is **denied by default** by the
//! permission engine (see `specs/browser-organ.md` §4).
//!
//! **Redirect boundary (#12-E):** the kernel's pre-dispatch gate classifies the
//! initial `url`, but redirects are followed *inside* Chrome where the kernel
//! never sees the next destination. To close that seam, every page the adapter
//! creates is armed with CDP `Fetch` interception for document navigations: a
//! redirect hop (or any other navigation-derived document request) is classified
//! against the caller's [`SecurityScope`] via [`crate::egress::EgressPolicy::check_url`]
//! before Chrome contacts it, and refused hops are failed with
//! `Fetch.failRequest` — the same `egress denied` / `egress_deny` contract the
//! HTTP adapter uses, which the runtime audits as `EgressRedirectDeny` and
//! cancels without retrying. The initial navigation is always continued: the
//! kernel owns that check, and direct callers pointed at local mock servers
//! must stay able to drive kernel-vetted destinations.
//!
//! **Credential guard:** `type` refuses `input[type=password]` structurally —
//! the browser profile is the credential boundary, so FRIDAY never types
//! credentials (specs/browser-organ.md §4).

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams, RequestId,
    RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::{ErrorReason, ResourceType};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::listeners::EventStream;
use chromiumoxide::page::{Page, ScreenshotParams};
use friday_agent_runtime::SecurityScope;
use futures::StreamExt;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::adapters::ActionAdapter;
use crate::egress::{EgressDecision, EgressDenyReason, EgressPolicy};
use crate::request::{ActionRequest, ActionResult};

/// Environment override for the Chrome/Chromium executable (chromiumoxide's
/// default launcher searches standard locations when unset).
const CHROME_PATH_ENV: &str = "FRIDAY_CHROME_PATH";
/// Set `1` to launch Chrome headed (defaults to headless).
const HEADED_ENV: &str = "FRIDAY_BROWSER_HEADED";

/// Live browser state: the browser plus the last opened page (session).
struct BrowserState {
    browser: Browser,
    page: Option<Page>,
    /// Shared interception state for the page's document requests.
    intercept: InterceptShared,
}

/// State shared between the adapter and the CDP interception task that answers
/// every paused document request on the current page.
#[derive(Clone)]
struct InterceptShared {
    /// The policy classification uses. Cloned from the adapter when the
    /// browser starts (the policy is immutable once configured).
    egress: EgressPolicy,
    /// The [`SecurityScope`] stamped by `open` for the current navigation.
    /// Click/type/submit-driven navigations reuse the most recently stamped
    /// scope — they belong to the same execution, so that is the correct
    /// authority for them too.
    scope: Arc<Mutex<Option<SecurityScope>>>,
    /// The initial URL of the current navigation, stamped by `open`. Any other
    /// document request is navigation-derived and must be classified.
    initial_url: Arc<Mutex<String>>,
    /// The first refused redirect hop of the current navigation, if any. Set
    /// *before* the request is failed, so by the time the navigation resolves
    /// (success or failure) the denial is already recorded.
    denied: Arc<Mutex<Option<EgressDenyReason>>>,
}

/// What to do with one paused document request: let Chrome continue it, or
/// fail it before the network is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterceptAction {
    Continue,
    Fail(EgressDenyReason),
}

/// Is a paused document request *navigation-derived* — a server redirect hop,
/// a sub-frame document load, or a JS/meta navigation — rather than the
/// initial URL the kernel already vetted?
///
/// The initial navigation is the only document request the kernel classifies
/// up front (the `AgentKernel::run_action` pre-dispatch gate), so every other
/// document request must be classified here. It is identified two ways so the
/// guard does not depend on a single CDP signal: the protocol's own
/// `redirectedRequestId` (set on requests that follow a redirect response),
/// and a URL comparison against the stamped initial target.
fn is_redirect_hop(
    initial_url: &str,
    redirected_request_id: Option<&RequestId>,
    url: &str,
) -> bool {
    redirected_request_id.is_some() || initial_url != url
}

/// Classify one paused document request against the caller's egress authority.
///
/// Only *navigation-derived* requests are classified: the initial navigation
/// is the kernel's pre-dispatch check and is always continued, so direct
/// callers may still drive kernel-vetted local destinations. The decision is
/// deliberately type-agnostic about *why* the request is a hop — it classifies
/// whatever destination the page is trying to reach. Document-only
/// *interception* is the scope boundary for this slice, not an architectural
/// rule: if a redirect ever surfaces under another resource type, only the
/// interception pattern needs widening, not this decision.
fn intercept_decision(
    egress: &EgressPolicy,
    scope: Option<&SecurityScope>,
    is_redirect_hop: bool,
    url: &str,
) -> InterceptAction {
    if !is_redirect_hop {
        return InterceptAction::Continue;
    }
    match egress.check_url(scope, url) {
        EgressDecision::Allow => InterceptAction::Continue,
        EgressDecision::Deny { reason } => InterceptAction::Fail(reason),
    }
}

/// The adapter's half of the egress-denial contract: a static error string
/// plus a static `egress_deny` classifier label (never the destination). The
/// runtime parses the label into `EgressRedirectDeny`, audits it, and cancels
/// the execution without retrying — exactly what the HTTP adapter reports.
fn deny_result(reason: EgressDenyReason) -> ActionResult {
    ActionResult::error("egress denied").with_metadata("egress_deny", json!(reason.as_str()))
}

/// Classify one paused document request and answer it: `Fetch.continueRequest`
/// (initial navigation or an allowed hop) or `Fetch.failRequest` (a refused
/// hop, with the denial recorded before Chrome's request is aborted). Errors
/// answering are swallowed deliberately — a failed answer leaves the request
/// paused, which aborts the navigation, which is the safe default.
async fn answer_paused(page: &Page, shared: &InterceptShared, ev: &EventRequestPaused) {
    let scope = shared.scope.lock().await.clone();
    let initial_url = shared.initial_url.lock().await.clone();
    let hop = is_redirect_hop(
        &initial_url,
        ev.redirected_request_id.as_ref(),
        &ev.request.url,
    );
    match intercept_decision(&shared.egress, scope.as_ref(), hop, &ev.request.url) {
        InterceptAction::Continue => {
            let _ = page
                .execute(ContinueRequestParams::new(ev.request_id.clone()))
                .await;
        }
        InterceptAction::Fail(reason) => {
            *shared.denied.lock().await = Some(reason);
            let _ = page
                .execute(FailRequestParams::new(
                    ev.request_id.clone(),
                    ErrorReason::BlockedByClient,
                ))
                .await;
        }
    }
}

/// Run the interception answering loop on its own thread.
///
/// The handler runs on a dedicated thread (not `tokio::spawn`) because
/// chromiumoxide's [`EventStream`] is not `Send`; the thread drives a private
/// tokio runtime. Commands are exchanged over runtime-agnostic channels, so
/// `Page` handles work from both the adapter task and this thread. The loop
/// exits when the page closes (its event channel closes), ending the thread.
fn arm_interception(
    page: Page,
    shared: InterceptShared,
    mut events: EventStream<EventRequestPaused>,
) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        rt.block_on(async move {
            while let Some(ev) = events.next().await {
                answer_paused(&page, &shared, &ev).await;
            }
        });
    });
}

pub struct BrowserAdapter {
    state: Mutex<Option<BrowserState>>,
    chrome_path: Option<PathBuf>,
    user_data_dir: Option<PathBuf>,
    headed: bool,
    /// Policy used to classify navigation-derived document requests (redirect
    /// hops) via CDP `Fetch` interception. Defaults to [`EgressPolicy::new`].
    egress: EgressPolicy,
    /// Extra Chrome command-line arguments (e.g. `--host-resolver-rules` for
    /// hermetic live tests).
    chrome_args: Vec<String>,
}

impl Default for BrowserAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserAdapter {
    pub fn new() -> Self {
        let chrome_path = std::env::var(CHROME_PATH_ENV).ok().map(PathBuf::from);
        let headed = std::env::var(HEADED_ENV).map(|v| v == "1").unwrap_or(false);
        Self {
            state: Mutex::new(None),
            chrome_path,
            user_data_dir: None,
            headed,
            egress: EgressPolicy::new(),
            chrome_args: Vec::new(),
        }
    }

    /// Configure the policy used to classify redirect hops and other
    /// navigation-derived document requests. Defaults to [`EgressPolicy::new`];
    /// direct users (outside the kernel's pre-dispatch gate) can harden with
    /// `with_deny_without_scope(true)` here.
    pub fn with_egress(mut self, egress: EgressPolicy) -> Self {
        self.egress = egress;
        self
    }

    /// Append a Chrome command-line argument. Test hosts use this to pin
    /// hostname resolution (e.g. `--host-resolver-rules=MAP friday.test 127.0.0.1`)
    /// so live redirect tests are hermetic without opening loopback egress.
    pub fn with_chrome_arg(mut self, arg: impl Into<String>) -> Self {
        self.chrome_args.push(arg.into());
        self
    }

    /// Attach to an externally-managed browser session (Chrome started with
    /// `--remote-debugging-port=9222`) instead of launching a browser here.
    pub fn with_connect_url(self, _url: String) -> Self {
        // Reserved seam: `Browser::connect` replaces the launch path. v1 uses
        // `user_data_dir` for session persistence; a remote-attach host can
        // wire this once the need for an out-of-process browser is proven.
        self
    }

    /// Persist cookies/localStorage in `dir` so sessions survive restarts.
    pub fn with_user_data_dir(mut self, dir: PathBuf) -> Self {
        self.user_data_dir = Some(dir);
        self
    }

    async fn launch(&self) -> Result<Browser, String> {
        let mut builder = BrowserConfig::builder()
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-gpu");
        for arg in &self.chrome_args {
            builder = builder.arg(arg.as_str());
        }
        if !self.headed {
            builder = builder.arg("--headless=new");
        }
        if let Some(path) = &self.chrome_path {
            builder = builder.chrome_executable(path.clone());
        }
        if let Some(dir) = &self.user_data_dir {
            builder = builder.user_data_dir(dir);
        }
        let config = builder
            .build()
            .map_err(|e| format!("browser config: {e}"))?;
        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("browser launch: {e}"))?;
        // The handler task drives CDP events; dropping it kills the connection.
        tokio::spawn(async move { while handler.next().await.is_some() {} });
        Ok(browser)
    }

    /// Create a page on `about:blank`, arm document interception, and return
    /// the page handle. Navigation is then driven via `Page::goto` so every
    /// redirect hop is classified before Chrome contacts it.
    ///
    /// Interception is scoped to `ResourceType::Document` at the request
    /// stage: only document navigations pause, so stylesheets/images/XHR/fetch
    /// keep flowing untouched (no interception overhead, and nothing can hang
    /// waiting on us). Document requests are the navigation boundary — the
    /// redirect seam that would otherwise be invisible to the egress policy.
    async fn new_page(browser: &Browser, shared: InterceptShared) -> Result<Page, String> {
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("browser new page: {e}"))?;
        let events = page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(|e| format!("browser request listener: {e}"))?;
        let pattern = RequestPattern::builder()
            .url_pattern("*")
            .resource_type(ResourceType::Document)
            .request_stage(RequestStage::Request)
            .build();
        page.execute(EnableParams::builder().pattern(pattern).build())
            .await
            .map_err(|e| format!("browser fetch enable: {e}"))?;
        arm_interception(page.clone(), shared, events);
        Ok(page)
    }

    async fn open(&self, req: &ActionRequest) -> ActionResult {
        let url = match req.param_str("url") {
            Ok(u) => u,
            Err(e) => return ActionResult::error(e),
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return ActionResult::error("only http/https URLs are allowed");
        }
        let mut state = self.state.lock().await;
        if state.is_none() {
            let browser = match self.launch().await {
                Ok(b) => b,
                Err(e) => return ActionResult::error(e),
            };
            *state = Some(BrowserState {
                browser,
                page: None,
                intercept: InterceptShared {
                    egress: self.egress.clone(),
                    scope: Arc::new(Mutex::new(None)),
                    initial_url: Arc::new(Mutex::new(String::new())),
                    denied: Arc::new(Mutex::new(None)),
                },
            });
        }
        let st = state.as_mut().expect("state initialized above");
        let page = if st.page.is_some() {
            st.page.as_ref().expect("checked above").clone()
        } else {
            let p = match Self::new_page(&st.browser, st.intercept.clone()).await {
                Ok(p) => p,
                Err(e) => return ActionResult::error(e),
            };
            st.page = Some(p.clone());
            p
        };
        // Stamp this navigation's authority and clear any prior denial before
        // navigating. The interception task classifies every hop against it.
        *st.intercept.scope.lock().await = req.security_scope.clone();
        *st.intercept.initial_url.lock().await = url.clone();
        *st.intercept.denied.lock().await = None;
        let nav = page.goto(&url).await;
        // A refused hop records the denial *before* failing the request, so by
        // the time the navigation resolves (success or failure) the reason is
        // already captured.
        if let Some(reason) = *st.intercept.denied.lock().await {
            return deny_result(reason);
        }
        if let Err(e) = nav {
            return ActionResult::error(format!("navigate {url}: {e}"));
        }
        let title = page.get_title().await.ok().flatten().unwrap_or_default();
        let current = page.url().await.unwrap_or_default();
        ActionResult::success(json!({ "title": title, "url": current }))
    }

    async fn extract(&self, req: &ActionRequest) -> ActionResult {
        let selector = req
            .parameters
            .get("selector")
            .and_then(|v| v.as_str())
            .map(String::from);
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        match selector {
            Some(sel) => match page.find_element(&sel).await {
                Ok(el) => match el.inner_text().await {
                    Ok(text) => ActionResult::success(json!({ "selector": sel, "text": text })),
                    Err(e) => ActionResult::error(format!("extract text: {e}")),
                },
                Err(e) => ActionResult::error(format!("find '{sel}': {e}")),
            },
            None => match page.content().await {
                Ok(html) => ActionResult::success(json!({ "content": html })),
                Err(e) => ActionResult::error(format!("extract content: {e}")),
            },
        }
    }

    async fn click(&self, req: &ActionRequest) -> ActionResult {
        let selector = match req.param_str("selector") {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        let el = match page.find_element(&selector).await {
            Ok(el) => el,
            Err(e) => return ActionResult::error(format!("find '{selector}': {e}")),
        };
        match el.click().await {
            Ok(_) => ActionResult::success(json!({ "clicked": selector })),
            Err(e) => ActionResult::error(format!("click '{selector}': {e}")),
        }
    }

    async fn typing(&self, req: &ActionRequest) -> ActionResult {
        let selector = match req.param_str("selector") {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let text = match req.param_str("text") {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        let el = match page.find_element(&selector).await {
            Ok(el) => el,
            Err(e) => return ActionResult::error(format!("find '{selector}': {e}")),
        };
        // Granular permission guard (specs/browser-organ.md §4): FRIDAY never
        // types credentials — the dedicated browser profile *is* the login.
        if let Ok(Some(ty)) = el.attribute("type").await {
            if ty.eq_ignore_ascii_case("password") {
                return ActionResult::error(
                    "refusing to type into a password field (browser profile is the credential boundary)",
                );
            }
        }
        // Focus the element first (chromiumoxide's documented click → type →
        // key flow): keystrokes and later key events land on the focused
        // element, which is what real input boxes expect.
        if let Err(e) = el.click().await {
            return ActionResult::error(format!("focus '{selector}': {e}"));
        }
        match el.type_str(&text).await {
            Ok(_) => ActionResult::success(json!({ "typed": selector })),
            Err(e) => ActionResult::error(format!("type into '{selector}': {e}")),
        }
    }

    async fn submit(&self, req: &ActionRequest) -> ActionResult {
        let selector = match req.param_str("selector") {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        let el = match page.find_element(&selector).await {
            Ok(el) => el,
            Err(e) => return ActionResult::error(format!("find '{selector}': {e}")),
        };
        match el.press_key("Enter").await {
            Ok(_) => ActionResult::success(json!({ "submitted": selector })),
            Err(e) => ActionResult::error(format!("submit '{selector}': {e}")),
        }
    }

    async fn evaluate(&self, req: &ActionRequest) -> ActionResult {
        let expression = match req.param_str("expression") {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        match page.evaluate(expression).await {
            Ok(ev) => match ev.into_value::<serde_json::Value>() {
                Ok(v) => ActionResult::success(json!({ "result": v })),
                Err(e) => ActionResult::error(format!("evaluate result: {e}")),
            },
            Err(e) => ActionResult::error(format!("evaluate: {e}")),
        }
    }

    async fn scroll(&self, req: &ActionRequest) -> ActionResult {
        let delta_y = req
            .parameters
            .get("delta_y")
            .and_then(|v| v.as_i64())
            .unwrap_or(400);
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        let js = format!("window.scrollBy(0, {delta_y});");
        match page.evaluate(js).await {
            Ok(_) => ActionResult::success(json!({ "delta_y": delta_y })),
            Err(e) => ActionResult::error(format!("scroll: {e}")),
        }
    }

    async fn screenshot(&self, req: &ActionRequest) -> ActionResult {
        let path = match req.param_str("path") {
            Ok(p) => p,
            Err(e) => return ActionResult::error(e),
        };
        let state = self.state.lock().await;
        let st = match state.as_ref() {
            Some(st) => st,
            None => return ActionResult::error("no browser — call web.open first"),
        };
        let page = match st.page.as_ref() {
            Some(p) => p,
            None => return ActionResult::error("no page open — call web.open first"),
        };
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .build();
        let png = match page.screenshot(params).await {
            Ok(png) => png,
            Err(e) => return ActionResult::error(format!("screenshot: {e}")),
        };
        if let Err(e) = std::fs::write(&path, &png) {
            return ActionResult::error(format!("write screenshot {path}: {e}"));
        }
        ActionResult::success(json!({ "path": path, "bytes": png.len() }))
    }
}

/// The capability records for the browser verbs, for runtime registration into
/// the planner's [`CapabilityRegistry`] (this is what flips the `web` skill's
/// soft-declared browser verbs to resolved). Hosts call
/// [`register_browser_capabilities`] when they construct a
/// [`BrowserAdapter`].
pub fn browser_capability_specs(
) -> Vec<friday_planner::execution::capability_registry::CapabilitySpec> {
    use friday_planner::execution::capability_registry::{CapabilitySpec, Risk};
    let mut specs = Vec::new();
    let mut push = |id: &str, name: &str, description: &str, action: &str, risk: Risk| {
        specs.push(CapabilitySpec::native(
            id.to_string(),
            name.to_string(),
            description.to_string(),
            "web".to_string(),
            action.to_string(),
            risk,
        ));
    };
    push(
        "web.open",
        "Open a URL",
        "Open a URL in the browser and wait for it to load",
        "open",
        Risk::Safe,
    );
    push(
        "web.extract",
        "Extract page content",
        "Extract the page HTML or an element's text by CSS selector",
        "extract",
        Risk::Safe,
    );
    push(
        "web.scroll",
        "Scroll the page",
        "Scroll the page by a pixel delta",
        "scroll",
        Risk::Safe,
    );
    push(
        "web.screenshot",
        "Screenshot the page",
        "Capture a PNG screenshot of the current page to a file path",
        "screenshot",
        Risk::Safe,
    );
    push(
        "web.click",
        "Click an element",
        "Click the first element matching a CSS selector",
        "click",
        Risk::Medium,
    );
    push(
        "web.type",
        "Type into an element",
        "Type text into the first element matching a CSS selector (never into a password field)",
        "type",
        Risk::Medium,
    );
    push(
        "web.submit",
        "Submit an element",
        "Press Enter on the first element matching a CSS selector",
        "submit",
        Risk::Medium,
    );
    push(
        "web.evaluate",
        "Run a JavaScript expression",
        "Execute a JavaScript expression in the current page and return its value (denied by default)",
        "evaluate",
        Risk::High,
    );
    specs
}

/// Register the browser verbs into the planner's capability registry. Call
/// alongside registering a [`BrowserAdapter`] with the kernel; the skill
/// registry's `reconcile` then flips the `web` skill's soft-declared verbs to
/// resolved, making the browser verbs selectable by the planner.
pub fn register_browser_capabilities(
    registry: &friday_planner::execution::capability_registry::CapabilityRegistry,
) {
    for spec in browser_capability_specs() {
        registry.register(spec);
    }
}

#[async_trait]
impl ActionAdapter for BrowserAdapter {
    fn tool(&self) -> &str {
        "web"
    }

    fn actions(&self) -> Vec<&str> {
        vec![
            "open",
            "click",
            "type",
            "submit",
            "scroll",
            "extract",
            "screenshot",
            "evaluate",
        ]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "open" => self.open(&req).await,
            "extract" => self.extract(&req).await,
            "click" => self.click(&req).await,
            "type" => self.typing(&req).await,
            "submit" => self.submit(&req).await,
            "scroll" => self.scroll(&req).await,
            "screenshot" => self.screenshot(&req).await,
            "evaluate" => self.evaluate(&req).await,
            other => ActionResult::error(format!("unknown web browser action: {other}")),
        }
    }
}

/// Serializes live browser tests so two headless Chrome instances never launch
/// concurrently — parallel launches are flaky on dev machines. Held only inside
/// the env-gated integration tests (`FRIDAY_BROWSER_TEST=1`).
#[cfg(test)]
pub(crate) static BROWSER_LIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    // Live-browser tests intentionally hold a std Mutex across await to serialize
    // real Chrome access; serialization is the point, not a bug.
    #![allow(clippy::await_holding_lock)]
    use super::*;

    fn adapter() -> BrowserAdapter {
        BrowserAdapter::new()
    }

    #[tokio::test]
    async fn rejects_non_http_urls() {
        let res = adapter()
            .execute(ActionRequest::new(
                "web",
                "open",
                json!({ "url": "file:///etc/passwd" }),
            ))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("http"));
    }

    #[tokio::test]
    async fn actions_without_browser_fail_gracefully() {
        let res = adapter()
            .execute(ActionRequest::new(
                "web",
                "screenshot",
                json!({ "path": "x.png" }),
            ))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("web.open"));

        let res = adapter()
            .execute(ActionRequest::new(
                "web",
                "click",
                json!({ "selector": "a" }),
            ))
            .await;
        assert!(!res.success);
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let res = adapter()
            .execute(ActionRequest::new("web", "teleport", json!({})))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("unknown"));
    }

    fn scope(egress: &[&str]) -> SecurityScope {
        SecurityScope::new(egress, &[])
    }

    #[test]
    fn hop_detection_uses_both_cdp_signals() {
        // The initial navigation is the only non-hop document request.
        assert!(!is_redirect_hop(
            "http://friday.test:1234/public",
            None,
            "http://friday.test:1234/public"
        ));
        // A redirect hop is caught by the protocol's redirectedRequestId...
        assert!(is_redirect_hop(
            "http://friday.test:1234/public",
            Some(&RequestId::new("7")),
            "http://friday.test:1234/public"
        ));
        // ...and independently by the URL differing from the stamped target,
        // so the guard does not depend on a single CDP signal.
        assert!(is_redirect_hop(
            "http://friday.test:1234/public",
            None,
            "http://127.0.0.1:1234/private"
        ));
    }

    #[test]
    fn initial_navigation_is_always_continued() {
        let p = EgressPolicy::new().with_deny_without_scope(true);
        // Even a destination that would be denied as a hop is continued as the
        // initial navigation: the kernel owns that check.
        assert_eq!(
            intercept_decision(&p, Some(&scope(&[])), false, "http://127.0.0.1:1/x"),
            InterceptAction::Continue
        );
    }

    #[test]
    fn redirect_to_internal_network_is_failed_even_when_scope_grants_it() {
        let p = EgressPolicy::new();
        // A scope that explicitly grants the loopback literal cannot open it.
        let grants = scope(&["127.0.0.1", "friday.test"]);
        for url in [
            "http://127.0.0.1:9000/private?token=SECRET",
            "http://localhost:9000/admin",
            "http://192.168.1.10/x",
            "http://[::1]:9000/x",
        ] {
            assert_eq!(
                intercept_decision(&p, Some(&grants), true, url),
                InterceptAction::Fail(EgressDenyReason::InternalDestination),
                "internal redirect not refused: {url}"
            );
        }
    }

    #[test]
    fn redirect_outside_the_scope_is_failed() {
        let p = EgressPolicy::new();
        let covered = scope(&["friday.test"]);
        assert_eq!(
            intercept_decision(&p, Some(&covered), true, "http://other.test:9000/landing"),
            InterceptAction::Fail(EgressDenyReason::NotCoveredByScope)
        );
        // A lookalike host is not confidently inside the authority either.
        assert_eq!(
            intercept_decision(
                &p,
                Some(&covered),
                true,
                "http://notfriday.test:9000/landing"
            ),
            InterceptAction::Fail(EgressDenyReason::NotCoveredByScope)
        );
    }

    #[test]
    fn redirect_within_the_scope_is_allowed() {
        let p = EgressPolicy::new();
        let covered = scope(&["friday.test", "other.test"]);
        assert_eq!(
            intercept_decision(&p, Some(&covered), true, "http://other.test:9000/landing"),
            InterceptAction::Continue
        );
        assert_eq!(
            intercept_decision(&p, Some(&covered), true, "http://friday.test:9000/landing"),
            InterceptAction::Continue
        );
    }

    #[test]
    fn deny_result_speaks_the_c2_kernel_contract() {
        for reason in [
            EgressDenyReason::InternalDestination,
            EgressDenyReason::NotCoveredByScope,
            EgressDenyReason::NoScopeAttached,
        ] {
            let res = deny_result(reason);
            assert!(!res.success);
            assert_eq!(res.error.as_deref(), Some("egress denied"));
            assert_eq!(
                res.metadata.get("egress_deny").and_then(|v| v.as_str()),
                Some(reason.as_str()),
                "label mismatch for {reason:?}"
            );
        }
    }

    /// Live browser integration: typing into an `input[type=password]` is
    /// refused by the credential guard (specs/browser-organ.md §4). Skipped
    /// unless `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn typing_refuses_password_fields() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping password-guard integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><body><input type="password" id="pw"></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;
        let adapter = adapter();
        let res = adapter
            .execute(ActionRequest::new("web", "open", json!({ "url": base })))
            .await;
        assert!(res.success, "open failed: {:?}", res.error);
        let res = adapter
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": "#pw", "text": "hunter2" }),
            ))
            .await;
        assert!(!res.success, "typing into a password field must be refused");
        assert!(res.error.unwrap().contains("password"), "unexpected error");
        server.abort();
    }

    /// Live browser integration (headless Chrome required): open a local page,
    /// extract, click, type, scroll, screenshot. Skipped unless
    /// `FRIDAY_BROWSER_TEST=1` so hermetic CI stays green.
    #[tokio::test]
    async fn browser_roundtrip_open_extract_type_click_screenshot() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping browser integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Test Page</title></head>
            <body><h1>Hello FRIDAY</h1>
            <input id="name"><button id="go">Go</button><p id="out">initial</p>
            <script>document.getElementById('go').onclick = function(){
              document.getElementById('out').textContent = 'clicked';
            };</script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;
        let adapter = adapter();
        let res = adapter
            .execute(ActionRequest::new("web", "open", json!({ "url": base })))
            .await;
        assert!(res.success, "open failed: {:?}", res.error);
        assert_eq!(res.output["title"], "Test Page");

        let res = adapter
            .execute(ActionRequest::new("web", "extract", json!({})))
            .await;
        assert!(res.success, "extract failed: {:?}", res.error);
        assert!(res.output["content"]
            .as_str()
            .unwrap()
            .contains("Hello FRIDAY"));

        let res = adapter
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": "#name", "text": "Bud" }),
            ))
            .await;
        assert!(res.success, "type failed: {:?}", res.error);

        let res = adapter
            .execute(ActionRequest::new(
                "web",
                "click",
                json!({ "selector": "#go" }),
            ))
            .await;
        assert!(res.success, "click failed: {:?}", res.error);

        let res = adapter
            .execute(ActionRequest::new(
                "web",
                "scroll",
                json!({ "delta_y": 120 }),
            ))
            .await;
        assert!(res.success, "scroll failed: {:?}", res.error);

        let shot =
            std::env::temp_dir().join(format!("friday-browser-{}.png", uuid::Uuid::new_v4()));
        let res = adapter
            .execute(ActionRequest::new(
                "web",
                "screenshot",
                json!({ "path": shot.to_string_lossy() }),
            ))
            .await;
        assert!(res.success, "screenshot failed: {:?}", res.error);
        assert!(res.output["bytes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_file(&shot);
        server.abort();
    }

    /// Live browser integration (#12-E): a document redirect that lands on an
    /// internal network is failed by the CDP interception *before* Chrome
    /// contacts it. The initial URL is a non-loopback host (`friday.test`,
    /// pinned to the loopback server via `--host-resolver-rules`) so the
    /// proof is a real redirected navigation, not a loopback first hop. The
    /// request-counting server proves the denied target was never reached.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn browser_redirect_to_internal_is_blocked_and_not_contacted() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping browser egress integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let server = CountingServer::start(|port| {
            vec![
                (
                    "/public",
                    Route {
                        status: 302,
                        location: Some(format!("http://127.0.0.1:{port}/private?token=SECRET")),
                        body: String::new(),
                    },
                ),
                (
                    "/private",
                    Route {
                        status: 200,
                        location: None,
                        body: "<!doctype html><title>Private</title>".to_string(),
                    },
                ),
            ]
        })
        .await;
        let adapter = adapter().with_chrome_arg("--host-resolver-rules=MAP friday.test 127.0.0.1");
        let mut req = ActionRequest::new(
            "web",
            "open",
            json!({ "url": format!("http://friday.test:{}/public", server.addr.port()) }),
        );
        req.security_scope = Some(scope(&["friday.test"]));
        let res = adapter.execute(req).await;
        assert!(!res.success, "internal redirect must be refused");
        assert_eq!(res.error.as_deref(), Some("egress denied"));
        assert_eq!(
            res.metadata.get("egress_deny").and_then(|v| v.as_str()),
            Some("internal_destination")
        );
        assert_eq!(
            server.hits("/public").await,
            1,
            "initial navigation must succeed"
        );
        assert_eq!(
            server.hits("/private").await,
            0,
            "denied target was contacted"
        );
        server.handle.abort();
    }

    /// Live browser integration (#12-E): a redirect to a host inside the
    /// caller's scope is allowed through and the navigation completes.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn browser_allowed_cross_host_redirect_still_functions() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping browser egress integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let server = CountingServer::start(|port| {
            vec![
                (
                    "/public",
                    Route {
                        status: 302,
                        location: Some(format!("http://other.test:{port}/landing")),
                        body: String::new(),
                    },
                ),
                (
                    "/landing",
                    Route {
                        status: 200,
                        location: None,
                        body: "<!doctype html><title>Landed</title>".to_string(),
                    },
                ),
            ]
        })
        .await;
        let adapter = adapter().with_chrome_arg(
            "--host-resolver-rules=MAP friday.test 127.0.0.1, MAP other.test 127.0.0.1",
        );
        let mut req = ActionRequest::new(
            "web",
            "open",
            json!({ "url": format!("http://friday.test:{}/public", server.addr.port()) }),
        );
        req.security_scope = Some(scope(&["friday.test", "other.test"]));
        let res = adapter.execute(req).await;
        assert!(res.success, "allowed redirect failed: {:?}", res.error);
        assert_eq!(res.output["title"], "Landed");
        assert_eq!(server.hits("/public").await, 1);
        assert_eq!(server.hits("/landing").await, 1);
        server.handle.abort();
    }

    /// Live browser integration (#12-E): a redirect to a host outside the
    /// caller's scope is refused before Chrome contacts it.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn browser_out_of_scope_redirect_is_denied_before_contact() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping browser egress integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let server = CountingServer::start(|port| {
            vec![
                (
                    "/public",
                    Route {
                        status: 302,
                        location: Some(format!("http://other.test:{port}/landing")),
                        body: String::new(),
                    },
                ),
                (
                    "/landing",
                    Route {
                        status: 200,
                        location: None,
                        body: "<!doctype html><title>Landed</title>".to_string(),
                    },
                ),
            ]
        })
        .await;
        let adapter = adapter().with_chrome_arg("--host-resolver-rules=MAP friday.test 127.0.0.1");
        let mut req = ActionRequest::new(
            "web",
            "open",
            json!({ "url": format!("http://friday.test:{}/public", server.addr.port()) }),
        );
        req.security_scope = Some(scope(&["friday.test"]));
        let res = adapter.execute(req).await;
        assert!(!res.success, "out-of-scope redirect must be refused");
        assert_eq!(res.error.as_deref(), Some("egress denied"));
        assert_eq!(
            res.metadata.get("egress_deny").and_then(|v| v.as_str()),
            Some("destination_not_covered_by_scope")
        );
        assert_eq!(server.hits("/public").await, 1);
        assert_eq!(
            server.hits("/landing").await,
            0,
            "out-of-scope target was contacted"
        );
        server.handle.abort();
    }

    /// Live kernel→browser seam (#12 sweep): the full chain the redirect
    /// boundary exists for — `SecurityScope` → kernel → `BrowserAdapter` → CDP
    /// `Fetch` → redirect → `EgressPolicy`. The kernel classifies the initial
    /// URL (`friday.test`, in scope, pinned to the loopback server), the
    /// browser follows the 302, the interception fails the hop to an internal
    /// destination *before* Chrome contacts it, and the kernel audits the
    /// denial as `EgressRedirectDeny`, cancels the execution, and does not
    /// retry. The secret embedded in the denied target never reaches the error
    /// surface or the audit trail.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn kernel_audits_browser_redirect_denial_and_cancels() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping browser egress integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = BROWSER_LIVE.lock().unwrap_or_else(|e| e.into_inner());
        let secret = "sk-browser-secret-31337";
        let server = CountingServer::start(|port| {
            vec![
                (
                    "/public",
                    Route {
                        status: 302,
                        location: Some(format!("http://127.0.0.1:{port}/private?token={secret}")),
                        body: String::new(),
                    },
                ),
                (
                    "/private",
                    Route {
                        status: 200,
                        location: None,
                        body: "<!doctype html><title>Private</title>".to_string(),
                    },
                ),
            ]
        })
        .await;

        let kernel = Arc::new(
            crate::runtime::AgentKernel::new().with_timeout(std::time::Duration::from_secs(30)),
        );
        kernel
            .register_adapter(Box::new(
                adapter().with_chrome_arg("--host-resolver-rules=MAP friday.test 127.0.0.1"),
            ))
            .await;

        let c = crate::kernel::ExecutionContext::builder()
            .execution_id(uuid::Uuid::new_v4())
            .session_id(uuid::Uuid::new_v4())
            .security_scope(Some(scope(&["friday.test"])))
            .build();
        let execution_id = c.execution_id;
        let req = ActionRequest::new(
            "web",
            "open",
            json!({ "url": format!("http://friday.test:{}/public", server.addr.port()) }),
        );
        let res = kernel.run_action(req, c).await;

        assert!(!res.success, "internal redirect must be refused");
        assert_eq!(res.error.as_deref(), Some("egress denied"));
        assert_eq!(
            server.hits("/public").await,
            1,
            "initial navigation must succeed"
        );
        assert_eq!(
            server.hits("/private").await,
            0,
            "denied target was contacted"
        );
        let err = res.error.clone().unwrap_or_default();
        assert!(!err.contains(secret), "error leaked the secret: {err}");
        assert_eq!(
            kernel.kernel().lifecycle.state(execution_id),
            Some(crate::kernel::LifecycleState::Cancelled),
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
            !detail.contains("127.0.0.1"),
            "audit leaked the destination: {detail}"
        );
        server.handle.abort();
    }

    /// Minimal static HTTP server for the browser test (no dependencies). Binds
    /// in the test task (no runtime race), then serves the HTML on a loop.
    async fn serve_html(html: String) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            loop {
                if let Ok((mut sock, _)) = listener.accept().await {
                    let html = html.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );
                        let _ =
                            tokio::io::AsyncWriteExt::write_all(&mut sock, resp.as_bytes()).await;
                    });
                }
            }
        });
        (format!("http://{addr}/index.html"), handle)
    }
}

/// A tiny route table for the live redirect tests: each route maps a request
/// path to a status, optional `Location`, and body. Every request increments a
/// per-path hit counter so tests can prove a denied target was never contacted
/// and an allowed hop was followed exactly once.
#[cfg(test)]
struct CountingServer {
    addr: std::net::SocketAddr,
    hits: Arc<Mutex<std::collections::HashMap<String, usize>>>,
    handle: tokio::task::JoinHandle<()>,
}

#[cfg(test)]
#[derive(Clone)]
struct Route {
    status: u16,
    location: Option<String>,
    body: String,
}

#[cfg(test)]
impl CountingServer {
    async fn start(routes: impl FnOnce(u16) -> Vec<(&'static str, Route)>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let routes: std::collections::HashMap<String, Route> = routes(addr.port())
            .into_iter()
            .map(|(p, r)| (p.to_string(), r))
            .collect();
        let hits: Arc<Mutex<std::collections::HashMap<String, usize>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let hits_for_task = hits.clone();
        let handle = tokio::spawn(async move {
            loop {
                if let Ok((mut sock, _)) = listener.accept().await {
                    let routes = routes.clone();
                    let hits = hits_for_task.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                        let request_line = String::from_utf8_lossy(&buf);
                        let path = request_line
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/")
                            .to_string();
                        *hits.lock().await.entry(path.clone()).or_insert(0) += 1;
                        let route = routes.get(&path).cloned().unwrap_or(Route {
                            status: 404,
                            location: None,
                            body: "not found".to_string(),
                        });
                        let reason = match route.status {
                            302 => "Found",
                            404 => "Not Found",
                            _ => "OK",
                        };
                        let mut head = format!(
                            "HTTP/1.1 {} {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n",
                            route.status,
                            reason,
                            route.body.len()
                        );
                        if let Some(loc) = &route.location {
                            head.push_str(&format!("Location: {loc}\r\n"));
                        }
                        head.push_str("\r\n");
                        let _ =
                            tokio::io::AsyncWriteExt::write_all(&mut sock, head.as_bytes()).await;
                        let _ =
                            tokio::io::AsyncWriteExt::write_all(&mut sock, route.body.as_bytes())
                                .await;
                    });
                }
            }
        });
        Self { addr, hits, handle }
    }

    async fn hits(&self, path: &str) -> usize {
        *self.hits.lock().await.get(path).unwrap_or(&0)
    }
}
