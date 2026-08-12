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
//! allowlist/denylist in the policy layer.

use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

use crate::adapters::ActionAdapter;
use crate::request::{ActionRequest, ActionResult};

/// Default search endpoint (DuckDuckGo's HTML endpoint — no API key needed).
/// Callers override with the `endpoint` parameter; tests point it at a local
/// server so the suite stays hermetic.
pub const DEFAULT_SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// Hard cap on a fetched page body (defense in depth — the kernel also enforces
/// its own per-attempt timeout).
pub const MAX_FETCH_CHARS: usize = 200_000;

#[derive(Clone)]
pub struct WebAdapter {
    client: reqwest::Client,
    default_endpoint: String,
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
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("friday-web/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client build"),
            default_endpoint: default_endpoint.into(),
        }
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
        let body = match self.get_text(&url).await {
            Ok(b) => b,
            Err(e) => return ActionResult::error(e),
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
        let body = match self.get_text(&url).await {
            Ok(b) => b,
            Err(e) => return ActionResult::error(e),
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

    async fn get_text(&self, url: &str) -> Result<String, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("GET {url} failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("GET {url} returned status {}", resp.status()));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("read body of {url} failed: {e}"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(text.chars().take(MAX_FETCH_CHARS).collect())
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
