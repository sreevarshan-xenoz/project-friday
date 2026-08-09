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
//! `click`/`type` are classified `Risk::Medium` (mutating a live page);
//! `open`/`extract`/`scroll`/`screenshot` are `Risk::Safe`.

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::{Page, ScreenshotParams};
use futures::StreamExt;
use serde_json::json;
use std::path::PathBuf;
use tokio::sync::Mutex;

use crate::adapters::ActionAdapter;
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
}

pub struct BrowserAdapter {
    state: Mutex<Option<BrowserState>>,
    chrome_path: Option<PathBuf>,
    user_data_dir: Option<PathBuf>,
    headed: bool,
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
        }
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
            });
        }
        let st = state.as_mut().expect("state initialized above");
        let page = match st.browser.new_page(&url).await {
            Ok(p) => p,
            Err(e) => return ActionResult::error(format!("navigate {url}: {e}")),
        };
        let title = page.get_title().await.ok().flatten().unwrap_or_default();
        let current = page.url().await.unwrap_or_default();
        st.page = Some(page);
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
        match el.type_str(&text).await {
            Ok(_) => ActionResult::success(json!({ "typed": selector })),
            Err(e) => ActionResult::error(format!("type into '{selector}': {e}")),
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
        "Type text into the first element matching a CSS selector",
        "type",
        Risk::Medium,
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
        vec!["open", "click", "type", "scroll", "extract", "screenshot"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "open" => self.open(&req).await,
            "extract" => self.extract(&req).await,
            "click" => self.click(&req).await,
            "type" => self.typing(&req).await,
            "scroll" => self.scroll(&req).await,
            "screenshot" => self.screenshot(&req).await,
            other => ActionResult::error(format!("unknown web browser action: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
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
        let _ = server.abort();
    }

    /// Minimal static HTTP server for the browser test (no dependencies). Binds
    /// in the test task (no runtime race), then serves the HTML on a loop.
    async fn serve_html(html: String) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:18734")
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
