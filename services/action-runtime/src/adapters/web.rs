//! Web capability adapter (read-only, no browser).
//!
//! v1 of the `web` skill needs no Playwright/CDP: `web.search` queries a
//! search endpoint and `web.fetch` fetches a page and extracts readable text.
//! The interactive browser verbs (`web.open`, `web.click`, ...) stay
//! soft-declared by the bundled `web` skill until a browser runtime arrives —
//! this adapter deliberately refuses to pretend they exist.
//!
//! The adapter is read-only by construction (it only issues GET requests), so
//! it is classified `Risk::Safe` in the native capability table.
//!
//! **Trust & SSRF note:** params are host-supplied (the planner/spec decides
//! what to fetch) and the scheme guard only blocks non-http(s) URLs — it does
//! NOT block internal networks (localhost, cloud metadata, RFC1918 ranges).
//! This matches the platform's model (the permission layer is the boundary,
//! same as `filesystem.read_file`); a hardened deployment should add an
//! allowlist/denylist in the policy layer. Since #12-C2 the adapter also
//! re-checks every *redirect hop* against the egress policy before issuing the
//! next request, so a 302 that lands on `127.0.0.1`/RFC1918 (or outside the
//! attached security scope) is refused before it is ever contacted.

use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;
use url::Url;

use crate::adapters::ActionAdapter;
use crate::egress::{EgressDecision, EgressDenyReason, EgressPolicy};
use crate::request::{ActionRequest, ActionResult};

/// Default search endpoint (DuckDuckGo's HTML endpoint — no API key needed).
/// Callers override with the `endpoint` parameter; tests point it at a local
/// server so the suite stays hermetic.
pub const DEFAULT_SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// Hard cap on a fetched page body (defense in depth — the kernel also enforces
/// its own per-attempt timeout).
pub const MAX_FETCH_CHARS: usize = 200_000;

/// Redirect hops the adapter will follow before giving up. Mirrors reqwest's
/// default cap, now that redirects are resolved by the adapter itself so each
/// hop can be checked against the egress policy (#12-C2).
pub const MAX_REDIRECTS: usize = 10;

/// Why a fetch failed. Kept separate from the public error string so an
/// egress denial can carry a static classifier label (never the destination)
/// for the kernel to audit.
enum FetchError {
    Http(String),
    EgressDenied(EgressDenyReason),
}

#[derive(Clone)]
pub struct WebAdapter {
    client: reqwest::Client,
    default_endpoint: String,
    egress: EgressPolicy,
}

impl Default for WebAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebAdapter {
    pub fn new() -> Self {
        Self::with_endpoint(DEFAULT_SEARCH_ENDPOINT.to_string())
    }

    /// Build an adapter whose search uses `default_endpoint` unless a request
    /// overrides it with its own `endpoint` parameter. The daemon uses this to
    /// point both the dedicated search tool and the evidence seam at a local
    /// mock server in hermetic end-to-end tests.
    pub fn with_endpoint(default_endpoint: impl Into<String>) -> Self {
        Self {
            // Redirects are handled by the adapter's own loop (#12-C2) so each
            // hop is classified by the egress policy before the next request;
            // the client itself never auto-follows.
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("friday-web/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client build"),
            default_endpoint: default_endpoint.into(),
            egress: EgressPolicy::new(),
        }
    }

    /// Configure the policy used to classify redirect hops. Defaults to
    /// [`EgressPolicy::new`]; direct users (outside the kernel's pre-dispatch
    /// gate) can harden with `with_deny_without_scope(true)` here.
    pub fn with_egress(mut self, egress: EgressPolicy) -> Self {
        self.egress = egress;
        self
    }

    fn all_actions() -> &'static [&'static str] {
        &["search", "fetch"]
    }

    async fn search(&self, req: &ActionRequest) -> ActionResult {
        let query = match req.param_str("query") {
            Ok(q) => q,
            Err(e) => return ActionResult::error(e),
        };
        let endpoint = req
            .parameters
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_endpoint)
            .to_string();
        let limit = req
            .parameters
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        let url = format!("{endpoint}?q={}", urlencode(&query));
        let body = match self.get_text(req, &url).await {
            Ok(b) => b,
            Err(e) => return Self::to_result(e),
        };
        let hits = parse_search_results(&body, limit);
        if hits.is_empty() {
            return ActionResult::success(json!({
                "query": query,
                "results": [],
                "note": "no results parsed from the search endpoint"
            }));
        }
        ActionResult::success(json!({ "query": query, "results": hits }))
    }

    async fn fetch(&self, req: &ActionRequest) -> ActionResult {
        let url = match req.param_str("url") {
            Ok(u) => u,
            Err(e) => return ActionResult::error(e),
        };
        let max_chars = req
            .parameters
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(20_000)
            .min(MAX_FETCH_CHARS as u64) as usize;
        // Only http(s) is allowed — the adapter must never be tricked into
        // reading local files or other schemes through `url`.
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ActionResult::error(format!("unsupported url scheme: {url}"));
        }
        let body = match self.get_text(req, &url).await {
            Ok(b) => b,
            Err(e) => return Self::to_result(e),
        };
        let title = extract_title(&body);
        let text = strip_html(&body);
        let truncated: String = text.chars().take(max_chars).collect();
        let is_truncated = truncated.len() < text.len();
        ActionResult::success(json!({
            "url": url,
            "title": title,
            "text": truncated,
            "chars": truncated.len(),
            "truncated": is_truncated,
        }))
    }

    /// Fetch a URL, following redirects **by hand** so every hop is classified
    /// by the egress policy before the next request is sent (#12-C2).
    ///
    /// The initial URL is intentionally not classified here: the kernel's
    /// pre-dispatch gate owns that check, and direct callers (evidence seams
    /// pointed at local mock servers) must stay able to fetch a local URL. Only
    /// the *redirect targets* — which the kernel never sees — are checked.
    async fn get_text(&self, req: &ActionRequest, url: &str) -> Result<String, FetchError> {
        let mut current = url.to_string();
        for _hop in 0..=MAX_REDIRECTS {
            let resp = self
                .client
                .get(&current)
                .send()
                .await
                .map_err(|e| FetchError::Http(format!("GET {current} failed: {e}")))?;

            // Mirror reqwest's default redirect set: 301/302/303/307/308. Any
            // other 3xx is a final (non-success) response.
            let follow = matches!(resp.status().as_u16(), 301 | 302 | 303 | 307 | 308);
            if !follow {
                if !resp.status().is_success() {
                    return Err(FetchError::Http(format!(
                        "GET {current} returned status {}",
                        resp.status()
                    )));
                }
                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| FetchError::Http(format!("read body of {current} failed: {e}")))?;
                let text = String::from_utf8_lossy(&bytes).into_owned();
                return Ok(text.chars().take(MAX_FETCH_CHARS).collect());
            }

            // Resolve the next hop against the current URL, then check it.
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| FetchError::Http("redirect without a location header".into()))?;
            let base = Url::parse(&current)
                .map_err(|_| FetchError::Http("invalid redirect base url".into()))?;
            let next = base
                .join(location)
                .map_err(|_| FetchError::Http("invalid redirect location".into()))?;
            let decision = self
                .egress
                .check_url(req.security_scope.as_ref(), next.as_str());
            if let EgressDecision::Deny { reason } = decision {
                // Refused before the second request. The reason is a static
                // label; the denied target is never echoed.
                return Err(FetchError::EgressDenied(reason));
            }
            current = next.to_string();
        }
        Err(FetchError::Http("too many redirects".into()))
    }

    fn to_result(e: FetchError) -> ActionResult {
        match e {
            FetchError::Http(msg) => ActionResult::error(msg),
            FetchError::EgressDenied(reason) => ActionResult::error("egress denied")
                .with_metadata("egress_deny", serde_json::json!(reason.as_str())),
        }
    }
}

/// Percent-encode a string into a URL-safe component (RFC 3986 unreserved
/// chars stay literal; everything else becomes `%XX`). Used for the `q=`
/// search parameter and by the resolver-backed fetch tool to embed a target
/// URL inside a gateway prefix (`https://r.jina.ai/<encoded target>`). Keeps
/// the adapter dependency-free.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Extract `(title, url)` pairs from an HTML search-results page. Crude by
/// design: finds anchors with http(s) hrefs and their link text. Works for the
/// default HTML endpoint and any simple results page (tests serve canned HTML).
fn parse_search_results(html: &str, limit: usize) -> Vec<serde_json::Value> {
    let mut hits = Vec::new();
    let mut rest = html;
    while hits.len() < limit {
        let Some(start) = rest.find("<a") else {
            break;
        };
        // Unconditionally advance past the `<a` marker so a tag without the
        // expected shape (e.g. `<abbr>`, `<area>`) skips to the next anchor
        // instead of aborting the whole parse.
        rest = &rest[start + 2..];
        let Some(href_start) = rest.find("href=\"") else {
            continue;
        };
        let href_after = &rest[href_start + 6..];
        let Some(href_end) = href_after.find('"') else {
            continue;
        };
        let url = href_after[..href_end].to_string();
        let after_href = &href_after[href_end + 1..];
        // Jump to the closing '>' of the anchor tag, then to its `</a>`.
        let Some(tag_end) = after_href.find('>') else {
            continue;
        };
        let after_tag = &after_href[tag_end + 1..];
        let Some(close_pos) = after_tag.find("</a>") else {
            continue;
        };
        let text = strip_html(&after_tag[..close_pos]);
        let text = text.trim();
        // Advance past this anchor before the next loop iteration.
        rest = &after_tag[close_pos + 4..];
        if !text.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            hits.push(json!({ "title": text, "url": url }));
        }
    }
    hits
}

/// Extract the `<title>…</title>` inner text (tags stripped, whitespace
/// collapsed, entities decoded). Empty when absent — never an error. Lets the
/// normalized fetch evidence carry a real page title instead of an empty one.
///
/// Searches **ASCII-case-insensitively over the original bytes** — `to_lowercase`
/// is deliberately avoided because Unicode lowercasing can change byte length
/// (`İ` → 3 bytes, `ẞ` → 2), which would make a byte offset computed on the
/// lowercased copy invalid for slicing the original. `<title>`/`</title>` are
/// pure ASCII, and `<` (0x3C) can never be a UTF-8 continuation byte, so a
/// match position is always on a char boundary.
pub fn extract_title(html: &str) -> String {
    let bytes = html.as_bytes();
    let needle = b"<title>";
    let mut pos = 0;
    while pos + needle.len() <= bytes.len() {
        if bytes[pos..pos + needle.len()].eq_ignore_ascii_case(needle) {
            break;
        }
        pos += 1;
    }
    if pos + needle.len() > bytes.len() {
        return String::new();
    }
    let after = &html[pos + needle.len()..];
    let Some(close) = after
        .as_bytes()
        .windows(b"</title>".len())
        .position(|w| w.eq_ignore_ascii_case(b"</title>"))
    else {
        return String::new();
    };
    strip_html(&after[..close]).trim().to_string()
}

/// Strip scripts/styles and tags, decode the common entities, and collapse
/// whitespace — enough to turn a fetched page into readable text.
pub fn strip_html(html: &str) -> String {
    let s = html.to_string();
    let mut scrubbed = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(open) = rest.find('<') {
        scrubbed.push_str(&rest[..open]);
        let rest2 = &rest[open..];
        if rest2.starts_with("<script") || rest2.starts_with("<style") {
            // Drop the whole element (and its content) until its closing tag.
            let lower = rest2.to_lowercase();
            let close_tag = if rest2.starts_with("<script") {
                "</script>"
            } else {
                "</style>"
            };
            let close = lower.find(close_tag);
            match close {
                Some(i) => rest = &rest2[i + close_tag.len()..],
                None => break,
            }
            continue;
        }
        let Some(close) = rest2.find('>') else { break };
        rest = &rest2[close + 1..];
    }
    scrubbed.push_str(rest);
    let decoded = scrubbed
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ");
    let mut out = String::with_capacity(decoded.len());
    let mut prev_space = false;
    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[async_trait]
impl ActionAdapter for WebAdapter {
    fn tool(&self) -> &str {
        "web"
    }

    fn actions(&self) -> Vec<&str> {
        Self::all_actions().to_vec()
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "search" => self.search(&req).await,
            "fetch" => self.fetch(&req).await,
            other => ActionResult::error(format!("unknown web action: {other}")),
        }
    }
}

/// Capability record published to the Capability Intelligence registry.
pub fn capabilities() -> Vec<friday_capabilities::model::Capability> {
    friday_capabilities::builtins::web()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_only(html: &str) -> Vec<serde_json::Value> {
        parse_search_results(html, 5)
    }

    #[test]
    fn strips_tags_and_entities() {
        let html = "<html><head><style>body{color:red}</style></head><body><script>alert(1)</script><h1>  FRIDAY &amp; Friends </h1><p>Hello\n\n   world</p></body></html>";
        assert_eq!(strip_html(html), "FRIDAY & Friends Hello world");
    }

    #[test]
    fn parses_anchor_results() {
        let html = r#"<html><body>
            <a rel="nofollow" class="result__a" href="https://doc.rust-lang.org/book/">The Rust Book</a>
            <a href="https://example.com/other">Other page</a>
            <a href="/relative">Skip me (relative)</a>
        </body></html>"#;
        let hits = parse_only(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0]["title"], "The Rust Book");
        assert_eq!(hits[0]["url"], "https://doc.rust-lang.org/book/");
        assert_eq!(hits[1]["title"], "Other page");
    }

    #[test]
    fn non_anchor_prefix_tags_do_not_abort_the_parse() {
        // An `<abbr>` (or any `<a...`-prefixed tag without `href`) before the
        // first real result must be skipped, not abort the whole extraction.
        let html = r#"<html><body>
            <abbr title="HyperText">HTML</abbr>
            <a href="https://doc.rust-lang.org/book/">The Rust Book</a>
            <a href="https://example.com/other">Other page</a>
        </body></html>"#;
        let hits = parse_only(html);
        assert_eq!(hits.len(), 2, "hits: {hits:?}");
        assert_eq!(hits[0]["url"], "https://doc.rust-lang.org/book/");
        assert_eq!(hits[1]["url"], "https://example.com/other");
    }

    #[test]
    fn percent_encodes_queries() {
        assert_eq!(urlencode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(urlencode("rust docs"), "rust%20docs");
    }

    #[test]
    fn extracts_page_title() {
        assert_eq!(
            extract_title("<html><head><title>  The Rust Book </title></head></html>"),
            "The Rust Book"
        );
        // Entities are decoded and whitespace collapsed inside the title.
        assert_eq!(
            extract_title("<html><head><title>Rust &amp; Async</title></head></html>"),
            "Rust & Async"
        );
        // Missing or malformed titles are empty, never an error.
        assert_eq!(extract_title("<html><body>no title</body></html>"), "");
        assert_eq!(extract_title("<html><head><title>unterminated"), "");

        // Non-ASCII text before the title — whose Unicode lowercasing changes
        // byte length — must not panic or corrupt the byte offset.
        assert_eq!(
            extract_title(
                "<meta name=\"author\" content=\"İstanbul\"><title>Ünïcode Title</title>"
            ),
            "Ünïcode Title"
        );

        // Mixed-case tags are matched case-insensitively.
        assert_eq!(
            extract_title("<HTML><HEAD><TITLE>Mixed Case</TITLE></HEAD></HTML>"),
            "Mixed Case"
        );
    }

    #[test]
    fn rejects_non_http_urls() {
        let adapter = WebAdapter::new();
        let req = ActionRequest::new("web", "fetch", json!({ "url": "file:///etc/passwd" }));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(adapter.execute(req));
        assert!(!res.success);
        assert!(res.error.unwrap().contains("scheme"));
    }
}
