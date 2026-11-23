//! Web search support for FRIDAY (ordered chain of backends and sources citation).
//!
//! Chain of backends:
//! 1. SearXNG (only if `FRIDAY_SEARXNG_URL` is set: the person's own instance)
//! 2. Brave Search API (only if `BRAVE_SEARCH_API_KEY` is set; key is never logged)
//! 3. DuckDuckGo HTML (keyless; HTTP 202 or page with no results treated as failure)
//! 4. Wikipedia API (keyless encyclopedia search fallback)
//!
//! Privacy rule: web search sends the question to a search engine, so it happens
//! only when the person asks for it (`friday ask --web`, `friday-ctl goal --web`,
//! or explicit search prefixes), or asks something only the web can answer now
//! (the weather, the news, a score, a price: [`wants_live`], owner's decision
//! 2026-11-23, switch `live-web`). Only the typed question goes, never selected
//! text, the screen or memory, and never in private mode. Secrets are always
//! masked before transmission.
//!
//! Egress log: records provider as the backend that was actually contacted, one
//! entry per backend attempted (`kinds: ["search"]`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// One parsed web search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A successful web search response carrying the answering backend's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResponse {
    pub backend: String,
    pub results: Vec<SearchResult>,
}

/// Pluggable web search provider trait for testing and production.
#[async_trait::async_trait]
pub trait WebSearcher: Send + Sync {
    async fn search(&self, query: &str) -> Result<SearchResponse, String>;
}

/// A single search backend in the chain.
#[async_trait::async_trait]
pub trait SearchBackend: Send + Sync {
    /// Friendly name for prompt and Sources attribution (e.g. "SearXNG", "Brave", "DuckDuckGo", "Wikipedia").
    fn name(&self) -> &str;

    /// Provider identifier for egress.jsonl (e.g. "searxng", "brave", "duckduckgo", "wikipedia").
    fn provider_id(&self) -> &str;

    /// Whether this backend has the required configuration (e.g. env var) to be contacted.
    fn is_configured(&self) -> bool;

    /// Whether this backend targets a local service (e.g. localhost SearXNG).
    fn is_local(&self) -> bool {
        false
    }

    /// Run the search query.
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String>;
}

/// Read an environment variable, falling back to reading `friday.env` if present.
pub fn read_env_or_config(var: &str) -> Option<String> {
    if let Ok(v) = std::env::var(var) {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok()?;
    let env_file = config_dir.join("friday").join("friday.env");
    let content = std::fs::read_to_string(env_file).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == var {
                let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                // A key kept in the keyring is never the marker itself.
                if !val.is_empty() && val != crate::secrets::MARKER {
                    return Some(val);
                }
            }
        }
    }
    None
}

/// SearXNG search backend (active only when `FRIDAY_SEARXNG_URL` is set).
pub struct SearxngBackend {
    url: Option<String>,
    client: reqwest::Client,
}

impl SearxngBackend {
    pub fn new(url: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self { url, client }
    }

    pub fn with_client(url: Option<String>, client: reqwest::Client) -> Self {
        Self { url, client }
    }
}

#[async_trait::async_trait]
impl SearchBackend for SearxngBackend {
    fn name(&self) -> &str {
        "SearXNG"
    }

    fn provider_id(&self) -> &str {
        "searxng"
    }

    fn is_configured(&self) -> bool {
        self.url
            .as_ref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
    }

    fn is_local(&self) -> bool {
        self.url
            .as_ref()
            .map(|u| {
                u.contains("localhost")
                    || u.contains("127.0.0.1")
                    || u.contains("[::1]")
                    || u.contains("0.0.0.0")
            })
            .unwrap_or(false)
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let Some(url) = &self.url else {
            return Err("SearXNG is not configured".to_string());
        };
        let base = url.trim_end_matches('/');
        let encoded_query = urlencode(query);
        let req_url = format!("{base}/search?q={encoded_query}&format=json");

        let resp = self
            .client
            .get(&req_url)
            .send()
            .await
            .map_err(|e| format!("SearXNG request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("SearXNG returned HTTP {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("failed to read SearXNG response: {e}"))?;

        parse_searxng_results(&body, 5)
    }
}

/// Parse SearXNG JSON response: `results[].{title, url, content}`.
pub fn parse_searxng_results(json_str: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid SearXNG JSON: {e}"))?;
    let results = val
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| "missing 'results' array in SearXNG response".to_string())?;

    let mut hits = Vec::new();
    for item in results {
        if hits.len() >= limit {
            break;
        }
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let content = item
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !title.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            hits.push(SearchResult {
                title: strip_html_tags_and_entities(title),
                url: url.to_string(),
                snippet: strip_html_tags_and_entities(content),
            });
        }
    }
    if hits.is_empty() {
        return Err("no results from SearXNG".to_string());
    }
    Ok(hits)
}

/// Brave Search API backend (active only when `BRAVE_SEARCH_API_KEY` is set).
///
/// Privacy rule: The API key is NEVER printed or logged.
pub struct BraveBackend {
    api_key: Option<String>,
    endpoint: String,
    client: reqwest::Client,
}

impl BraveBackend {
    pub const DEFAULT_ENDPOINT: &'static str = "https://api.search.brave.com/res/v1/web/search";

    pub fn new(api_key: Option<String>) -> Self {
        Self::with_endpoint(api_key, Self::DEFAULT_ENDPOINT)
    }

    pub fn with_endpoint(api_key: Option<String>, endpoint: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self {
            api_key,
            endpoint: endpoint.into(),
            client,
        }
    }

    pub fn with_client(
        api_key: Option<String>,
        endpoint: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            api_key,
            endpoint: endpoint.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl SearchBackend for BraveBackend {
    fn name(&self) -> &str {
        "Brave"
    }

    fn provider_id(&self) -> &str {
        "brave"
    }

    fn is_configured(&self) -> bool {
        self.api_key
            .as_ref()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let Some(key) = &self.api_key else {
            return Err("Brave Search API key is not configured".to_string());
        };
        let encoded_query = urlencode(query);
        let sep = if self.endpoint.contains('?') {
            "&"
        } else {
            "?"
        };
        let req_url = format!("{}{sep}q={encoded_query}&count=5", self.endpoint);

        // Sanitize error strings to ensure the key is NEVER revealed.
        let sanitize = |err: String| {
            if !key.is_empty() {
                err.replace(key, "[REDACTED]")
            } else {
                err
            }
        };

        let resp = self
            .client
            .get(&req_url)
            .header("X-Subscription-Token", key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| sanitize(format!("Brave search request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(format!("Brave search returned HTTP {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| sanitize(format!("failed to read Brave response: {e}")))?;

        parse_brave_results(&body, 5)
    }
}

/// Parse Brave Search API JSON response: `web.results[].{title, url, description}`.
pub fn parse_brave_results(json_str: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid Brave JSON: {e}"))?;
    let results = val
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
        .ok_or_else(|| "missing 'web.results' array in Brave response".to_string())?;

    let mut hits = Vec::new();
    for item in results {
        if hits.len() >= limit {
            break;
        }
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !title.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            hits.push(SearchResult {
                title: strip_html_tags_and_entities(title),
                url: url.to_string(),
                snippet: strip_html_tags_and_entities(description),
            });
        }
    }
    if hits.is_empty() {
        return Err("no results from Brave".to_string());
    }
    Ok(hits)
}

/// DuckDuckGo HTML endpoint searcher (keyless).
///
/// HTTP 202 (anti-bot challenge) or empty results are treated as failures.
pub struct DuckDuckGoBackend {
    endpoint: String,
    client: reqwest::Client,
}

pub type DuckDuckGoSearcher = DuckDuckGoBackend;

impl Default for DuckDuckGoBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DuckDuckGoBackend {
    pub const DEFAULT_ENDPOINT: &'static str = "https://html.duckduckgo.com/html/";

    pub fn new() -> Self {
        Self::with_endpoint(Self::DEFAULT_ENDPOINT)
    }

    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0")
            .build()
            .unwrap_or_default();
        Self {
            client,
            endpoint: endpoint.into(),
        }
    }

    pub fn with_client(endpoint: impl Into<String>, client: reqwest::Client) -> Self {
        Self {
            endpoint: endpoint.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl SearchBackend for DuckDuckGoBackend {
    fn name(&self) -> &str {
        "DuckDuckGo"
    }

    fn provider_id(&self) -> &str {
        "duckduckgo"
    }

    fn is_configured(&self) -> bool {
        true
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let encoded_query = urlencode(query);
        let sep = if self.endpoint.contains('?') {
            "&"
        } else {
            "?"
        };
        let url = format!("{}{sep}q={encoded_query}", self.endpoint);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("duckduckgo request failed: {e}"))?;

        // HTTP 202 is an anti-bot challenge page; treat as failure to fall through.
        if resp.status().as_u16() == 202 || !resp.status().is_success() {
            return Err(format!("duckduckgo returned HTTP {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("failed to read duckduckgo response: {e}"))?;

        let hits = parse_ddg_results(&body, 5);
        if hits.is_empty() {
            return Err("no results parsed from duckduckgo endpoint".to_string());
        }
        Ok(hits)
    }
}

/// Wikipedia search API backend (keyless).
pub struct WikipediaBackend {
    endpoint: String,
    client: reqwest::Client,
    user_agent: String,
}

impl Default for WikipediaBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WikipediaBackend {
    pub const DEFAULT_ENDPOINT: &'static str = "https://en.wikipedia.org";

    pub fn new() -> Self {
        Self::with_endpoint(Self::DEFAULT_ENDPOINT)
    }

    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        let user_agent = format!(
            "FRIDAY/{} (desktop assistant; https://github.com/sreevarshan-xenoz/project-friday)",
            env!("CARGO_PKG_VERSION")
        );
        Self {
            endpoint: endpoint.into(),
            client,
            user_agent,
        }
    }

    pub fn with_client(endpoint: impl Into<String>, client: reqwest::Client) -> Self {
        let user_agent = format!(
            "FRIDAY/{} (desktop assistant; https://github.com/sreevarshan-xenoz/project-friday)",
            env!("CARGO_PKG_VERSION")
        );
        Self {
            endpoint: endpoint.into(),
            client,
            user_agent,
        }
    }
}

#[async_trait::async_trait]
impl SearchBackend for WikipediaBackend {
    fn name(&self) -> &str {
        "Wikipedia"
    }

    fn provider_id(&self) -> &str {
        "wikipedia"
    }

    fn is_configured(&self) -> bool {
        true
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let base = self.endpoint.trim_end_matches('/');
        let encoded_query = urlencode(query);
        let req_url = format!(
            "{base}/w/api.php?action=query&list=search&srsearch={encoded_query}&format=json&srlimit=5"
        );

        let resp = self
            .client
            .get(&req_url)
            .header("User-Agent", &self.user_agent)
            .send()
            .await
            .map_err(|e| format!("wikipedia request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("wikipedia returned HTTP {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("failed to read wikipedia response: {e}"))?;

        parse_wikipedia_results(&body, 5)
    }
}

/// Parse Wikipedia search JSON response:
/// `query.search[].{title, snippet}` -> `title`, `https://en.wikipedia.org/wiki/<title_with_underscore>`, snippet with HTML stripped.
pub fn parse_wikipedia_results(json_str: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid Wikipedia JSON: {e}"))?;
    let search = val
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(|s| s.as_array())
        .ok_or_else(|| "missing 'query.search' array in Wikipedia response".to_string())?;

    let mut hits = Vec::new();
    for item in search {
        if hits.len() >= limit {
            break;
        }
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let raw_snippet = item
            .get("snippet")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !title.is_empty() {
            let clean_title = strip_html_tags_and_entities(title);
            // `?` and `#` would end the path; `%` would start an escape.
            let wiki_slug = clean_title
                .replace(' ', "_")
                .replace('%', "%25")
                .replace('?', "%3F")
                .replace('#', "%23");
            let url = format!("https://en.wikipedia.org/wiki/{wiki_slug}");
            let snippet = strip_html_tags_and_entities(raw_snippet);
            hits.push(SearchResult {
                title: clean_title,
                url,
                snippet,
            });
        }
    }
    if hits.is_empty() {
        return Err("no results from Wikipedia".to_string());
    }
    Ok(hits)
}

/// Chained web searcher: searches backends in priority order and returns the first with results.
pub struct ChainedWebSearcher {
    backends: Vec<Arc<dyn SearchBackend>>,
    egress_log_path: PathBuf,
}

impl Default for ChainedWebSearcher {
    fn default() -> Self {
        Self::default_chain()
    }
}

impl ChainedWebSearcher {
    /// Standard production search chain: SearXNG (if configured) -> Brave (if configured) -> DuckDuckGo -> Wikipedia.
    pub fn default_chain() -> Self {
        let searxng_url = read_env_or_config("FRIDAY_SEARXNG_URL");
        let brave_key = read_env_or_config("BRAVE_SEARCH_API_KEY");
        let backends: Vec<Arc<dyn SearchBackend>> = vec![
            Arc::new(SearxngBackend::new(searxng_url)),
            Arc::new(BraveBackend::new(brave_key)),
            Arc::new(DuckDuckGoBackend::new()),
            Arc::new(WikipediaBackend::new()),
        ];
        Self {
            backends,
            egress_log_path: if cfg!(test) {
                std::env::temp_dir()
                    .join(format!("friday-test-egress-{}.jsonl", std::process::id()))
            } else {
                friday_intelligence::default_egress_path()
            },
        }
    }

    /// Construct with custom backends.
    pub fn with_backends(backends: Vec<Arc<dyn SearchBackend>>) -> Self {
        Self {
            backends,
            egress_log_path: if cfg!(test) {
                std::env::temp_dir()
                    .join(format!("friday-test-egress-{}.jsonl", std::process::id()))
            } else {
                friday_intelligence::default_egress_path()
            },
        }
    }

    /// Override egress log path (useful for hermetic tests).
    pub fn with_egress_log_path(mut self, path: PathBuf) -> Self {
        self.egress_log_path = path;
        self
    }
}

#[async_trait::async_trait]
impl WebSearcher for ChainedWebSearcher {
    async fn search(&self, query: &str) -> Result<SearchResponse, String> {
        let (masked_query, _) = friday_goal_execution::mask_secrets(query);
        if masked_query.trim().is_empty() {
            return Err("empty search query".to_string());
        }

        for backend in &self.backends {
            if !backend.is_configured() {
                continue;
            }
            let outcome_result = backend.search(&masked_query).await;
            let (is_ok, hits) = match &outcome_result {
                Ok(h) if !h.is_empty() => (true, Some(h.clone())),
                _ => (false, None),
            };

            // Log attempt to egress log:
            // "Egress log provider = the backend that was actually contacted, one entry per backend attempted (kinds ["search"])."
            let ctx = friday_intelligence::current_egress_context();
            let entry = friday_intelligence::EgressEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                goal_id: ctx.goal_id,
                provider: backend.provider_id().to_string(),
                model: "-".to_string(),
                kinds: vec!["search".to_string()],
                text_chars: masked_query.chars().count(),
                image_bytes: 0,
                masked: ctx.masked,
                outcome: if is_ok {
                    "ok".to_string()
                } else {
                    "error".to_string()
                },
                local: if backend.is_local() { Some(true) } else { None },
                ..Default::default()
            };
            friday_intelligence::append_entry(&self.egress_log_path, &entry);

            if let Some(results) = hits {
                return Ok(SearchResponse {
                    backend: backend.name().to_string(),
                    results,
                });
            }
        }

        Err("all search backends failed".to_string())
    }
}

/// Percent-encode a query string safely.
/// Whether a question asks about something only the web knows now: the
/// weather, the news, a score, a price ("how is the weather today in
/// Chennai?"). Such an ask searches the web without "search …" first
/// (`live-web`); one about this computer or the person's own things doesn't.
pub fn wants_live(question: &str) -> bool {
    let words: String = format!(" {} ", question.to_lowercase())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    let has = |w: &str| words.contains(&format!(" {w} "));
    // This computer, the person's things, or what is in front of them: answered
    // here, not from the web.
    const MINE: [&str; 14] = [
        "cpu", "gpu", "battery", "laptop", "computer", "pc", "file", "folder", "screen", "disk",
        "fan", "article", "page", "this",
    ];
    // How something works isn't news ("how do RSS news feeds work").
    const HOW: [&str; 6] = [
        " how do ",
        " how does ",
        " explain ",
        " what is a ",
        " why do ",
        " how to ",
    ];
    if MINE.iter().any(|w| has(w)) || HOW.iter().any(|h| words.starts_with(h)) {
        return false;
    }
    const ALWAYS: [&str; 10] = [
        "weather",
        "forecast",
        "humidity",
        "headlines",
        "news",
        "aqi",
        "raining",
        "snowing",
        "sunny",
        "monsoon",
    ];
    if ALWAYS.iter().any(|w| has(w))
        || [
            " will it rain ",
            " air quality ",
            " who won ",
            " stock price ",
            " exchange rate ",
            " price of ",
        ]
        .iter()
        .any(|p| words.contains(p))
    {
        return true;
    }
    let now = [
        "today",
        "tonight",
        "now",
        "currently",
        "yesterday",
        "latest",
        "live",
    ]
    .iter()
    .any(|w| has(w));
    // A temperature or a score is the web's only with a place or a time.
    const TIMELY: [&str; 10] = [
        "score",
        "won",
        "price",
        "stock",
        "rate",
        "match",
        "election",
        "result",
        "results",
        "temperature",
    ];
    TIMELY.iter().any(|w| has(w)) && (now || (has("temperature") && has("in")))
}

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

/// Percent-decode a string (handles %XX).
pub fn percent_decode(s: &str) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(hex) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
            {
                bytes.push(hex);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Unwrap DuckDuckGo's `/l/?uddg=` redirect URLs to the target URL.
pub fn unwrap_ddg_url(raw: &str) -> String {
    let clean = raw.replace("&amp;", "&");
    if let Some(pos) = clean.find("uddg=") {
        let after = &clean[pos + 5..];
        let end = after.find('&').unwrap_or(after.len());
        let val = &after[..end];
        if let Ok(decoded) = percent_decode(val) {
            if decoded.starts_with("http://") || decoded.starts_with("https://") {
                return decoded;
            }
        }
    }
    if clean.starts_with("//") {
        return format!("https:{clean}");
    }
    clean
}

/// Strip HTML tags, unescape common entities, and collapse whitespace.
pub fn strip_html_tags_and_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    let decoded = out
        .replace("&amp;", "&")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let mut collapsed = String::with_capacity(decoded.len());
    let mut prev_space = false;
    for c in decoded.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }
    collapsed.trim().to_string()
}

/// Parse up to `limit` search results from DuckDuckGo HTML output.
pub fn parse_ddg_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut pos = 0;

    while results.len() < limit && pos < html.len() {
        let Some(a_class_offset) = html[pos..].find("result__a") else {
            break;
        };
        let a_class_pos = pos + a_class_offset;

        let Some(a_start_rel) = html[pos..a_class_pos].rfind("<a") else {
            pos = a_class_pos + 8;
            continue;
        };
        let a_start = pos + a_start_rel;

        let Some(a_end_rel) = html[a_start..].find("</a>") else {
            break;
        };
        let a_end = a_start + a_end_rel + 4;
        let a_elem = &html[a_start..a_end];

        let raw_url = if let Some(href_pos) = a_elem.find("href=\"") {
            let after = &a_elem[href_pos + 6..];
            if let Some(href_end) = after.find('"') {
                after[..href_end].to_string()
            } else {
                String::new()
            }
        } else if let Some(href_pos) = a_elem.find("href='") {
            let after = &a_elem[href_pos + 6..];
            if let Some(href_end) = after.find('\'') {
                after[..href_end].to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let title = strip_html_tags_and_entities(a_elem);
        let url = unwrap_ddg_url(&raw_url);

        let next_title_offset = html[a_end..].find("result__a");
        let snippet_bound = match next_title_offset {
            Some(rel) => a_end + rel,
            None => html.len(),
        };

        let mut snippet = String::new();
        if let Some(snip_rel) = html[a_end..snippet_bound].find("result__snippet") {
            let snip_start = a_end + snip_rel;
            if let Some(tag_close) = html[snip_start..snippet_bound].find('>') {
                let snip_inner_start = snip_start + tag_close + 1;
                let end_markers = ["</a>", "</div>", "</span>", "</p>"];
                let mut best_end = snippet_bound;
                for marker in &end_markers {
                    if let Some(em) = html[snip_inner_start..snippet_bound].find(marker) {
                        best_end = best_end.min(snip_inner_start + em);
                    }
                }
                snippet = strip_html_tags_and_entities(&html[snip_inner_start..best_end]);
            }
        }

        if !title.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        pos = a_end;
    }

    results
}

/// Append the "Sources (<Backend>):" section to the answer for cited sources (or all if none cited).
pub fn append_sources(answer: &str, backend: &str, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return answer.to_string();
    }

    let mut cited: Vec<usize> = Vec::new();
    for (i, _) in results.iter().enumerate() {
        let marker = format!("[{}]", i + 1);
        if answer.contains(&marker) {
            cited.push(i);
        }
    }

    // Fall back to all if none cited
    if cited.is_empty() {
        cited = (0..results.len()).collect();
    }

    let mut out = answer.trim_end().to_string();
    let header = if backend.is_empty() {
        "Sources:".to_string()
    } else {
        format!("Sources ({backend}):")
    };
    out.push_str(&format!("\n\n{header}\n"));
    for (pos, idx) in cited.iter().enumerate() {
        if pos > 0 {
            out.push('\n');
        }
        let r = &results[*idx];
        out.push_str(&format!("[{}] {} — {}", idx + 1, r.title, r.url));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SEARXNG_JSON: &str = r#"{
      "query": "rust programming",
      "number_of_results": 2,
      "results": [
        {
          "title": "Rust Programming Language",
          "url": "https://www.rust-lang.org/",
          "content": "A language empowering everyone to build reliable and efficient software."
        },
        {
          "title": "The Rust Reference",
          "url": "https://doc.rust-lang.org/reference/",
          "content": "Primary reference manual for the Rust language."
        }
      ]
    }"#;

    const FIXTURE_BRAVE_JSON: &str = r#"{
      "query": {
        "original": "rust programming"
      },
      "web": {
        "results": [
          {
            "title": "Rust Programming Language",
            "url": "https://www.rust-lang.org/",
            "description": "A language empowering everyone to build reliable and efficient software."
          },
          {
            "title": "Rust on GitHub",
            "url": "https://github.com/rust-lang/rust",
            "description": "Empowering everyone to build reliable and efficient software."
          }
        ]
      }
    }"#;

    const FIXTURE_WIKIPEDIA_JSON: &str = r#"{
      "batchcomplete": "",
      "query": {
        "search": [
          {
            "ns": 0,
            "title": "Rust (programming language)",
            "pageid": 29414838,
            "size": 123456,
            "wordcount": 12345,
            "snippet": "<span class=\"searchmatch\">Rust</span> is a general-purpose programming language. It emphasizes <span class=\"searchmatch\">safety</span> and performance.",
            "timestamp": "2026-11-01T00:00:00Z"
          },
          {
            "ns": 0,
            "title": "Rust Foundation",
            "pageid": 66728341,
            "size": 45678,
            "wordcount": 3456,
            "snippet": "The <span class=\"searchmatch\">Rust</span> Foundation is an independent non-profit organization &amp; steward.",
            "timestamp": "2026-11-02T00:00:00Z"
          }
        ]
      }
    }"#;

    const FIXTURE_DDG_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Search Results</title></head>
<body>
  <div class="result results_links results_links_deep web-result ">
    <div class="links_main links_deep result__body">
      <h2 class="result__title">
        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fstable%2Freleases.html&amp;rut=12345">Rust &amp; Release Notes</a>
      </h2>
      <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fstable%2Freleases.html&amp;rut=12345">On nightly <b>Rust</b>, it is possible to get releases.</a>
    </div>
  </div>
  <div class="result results_links results_links_deep web-result ">
    <div class="links_main links_deep result__body">
      <h2 class="result__title">
        <a rel="nofollow" class="result__a" href="https://github.com/rust-lang/rust/releases">GitHub Releases</a>
      </h2>
      <a class="result__snippet" href="https://github.com/rust-lang/rust/releases">Empowering everyone to build reliable software.</a>
    </div>
  </div>
</body>
</html>
"#;

    #[test]
    fn parses_searxng_fixture_correctly() {
        let results = parse_searxng_results(FIXTURE_SEARXNG_JSON, 5).expect("parse searxng");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].snippet,
            "A language empowering everyone to build reliable and efficient software."
        );
        assert_eq!(results[1].title, "The Rust Reference");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/reference/");
    }

    #[test]
    fn parses_brave_fixture_correctly() {
        let results = parse_brave_results(FIXTURE_BRAVE_JSON, 5).expect("parse brave");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].snippet,
            "A language empowering everyone to build reliable and efficient software."
        );
        assert_eq!(results[1].title, "Rust on GitHub");
        assert_eq!(results[1].url, "https://github.com/rust-lang/rust");
    }

    #[test]
    fn wikipedia_titles_with_a_question_mark_or_hash_make_working_links() {
        let json = r#"{"query":{"search":[
            {"title":"Who's That Girl?","snippet":"a film"},
            {"title":"C#","snippet":"a language"},
            {"title":"AC/DC","snippet":"a band"}]}}"#;
        let urls: Vec<String> = parse_wikipedia_results(json, 5)
            .expect("parse")
            .into_iter()
            .map(|r| r.url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://en.wikipedia.org/wiki/Who's_That_Girl%3F",
                "https://en.wikipedia.org/wiki/C%23",
                "https://en.wikipedia.org/wiki/AC/DC",
            ]
        );
    }

    #[test]
    fn parses_wikipedia_fixture_with_html_snippets() {
        let results = parse_wikipedia_results(FIXTURE_WIKIPEDIA_JSON, 5).expect("parse wikipedia");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust (programming language)");
        assert_eq!(
            results[0].url,
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(
            results[0].snippet,
            "Rust is a general-purpose programming language. It emphasizes safety and performance."
        );

        assert_eq!(results[1].title, "Rust Foundation");
        assert_eq!(
            results[1].url,
            "https://en.wikipedia.org/wiki/Rust_Foundation"
        );
        assert_eq!(
            results[1].snippet,
            "The Rust Foundation is an independent non-profit organization & steward."
        );
    }

    #[test]
    fn parses_ddg_fixture_correctly() {
        let results = parse_ddg_results(FIXTURE_DDG_HTML, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust & Release Notes");
        assert_eq!(
            results[0].url,
            "https://doc.rust-lang.org/stable/releases.html"
        );
        assert_eq!(
            results[0].snippet,
            "On nightly Rust, it is possible to get releases."
        );
    }

    struct MockBackend {
        name: &'static str,
        provider_id: &'static str,
        configured: bool,
        is_local: bool,
        response: Result<Vec<SearchResult>, String>,
        contacted: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl SearchBackend for MockBackend {
        fn name(&self) -> &str {
            self.name
        }

        fn provider_id(&self) -> &str {
            self.provider_id
        }

        fn is_configured(&self) -> bool {
            self.configured
        }

        fn is_local(&self) -> bool {
            self.is_local
        }

        async fn search(&self, _query: &str) -> Result<Vec<SearchResult>, String> {
            self.contacted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.response.clone()
        }
    }

    #[tokio::test]
    async fn chain_order_configured_searxng_first() {
        let searxng_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let brave_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ddg_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wiki_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let searxng = Arc::new(MockBackend {
            name: "SearXNG",
            provider_id: "searxng",
            configured: true,
            is_local: true,
            response: Ok(vec![SearchResult {
                title: "SearXNG Hit".to_string(),
                url: "https://example.com/searxng".to_string(),
                snippet: "From SearXNG".to_string(),
            }]),
            contacted: searxng_contacted.clone(),
        });
        let brave = Arc::new(MockBackend {
            name: "Brave",
            provider_id: "brave",
            configured: true,
            is_local: false,
            response: Ok(vec![]),
            contacted: brave_contacted.clone(),
        });
        let ddg = Arc::new(MockBackend {
            name: "DuckDuckGo",
            provider_id: "duckduckgo",
            configured: true,
            is_local: false,
            response: Ok(vec![]),
            contacted: ddg_contacted.clone(),
        });
        let wiki = Arc::new(MockBackend {
            name: "Wikipedia",
            provider_id: "wikipedia",
            configured: true,
            is_local: false,
            response: Ok(vec![]),
            contacted: wiki_contacted.clone(),
        });

        let tmp_egress = std::env::temp_dir().join(format!(
            "test_egress_chain_{}.jsonl",
            uuid::Uuid::new_v4().simple()
        ));
        let searcher = ChainedWebSearcher::with_backends(vec![searxng, brave, ddg, wiki])
            .with_egress_log_path(tmp_egress.clone());

        let res = searcher
            .search("who maintains rust")
            .await
            .expect("searxng succeeds");
        assert_eq!(res.backend, "SearXNG");
        assert_eq!(res.results.len(), 1);
        assert_eq!(res.results[0].title, "SearXNG Hit");

        assert!(searxng_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!brave_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!ddg_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!wiki_contacted.load(std::sync::atomic::Ordering::SeqCst));

        // Check egress log entry
        let content = std::fs::read_to_string(&tmp_egress).unwrap();
        assert!(content.contains("\"provider\":\"searxng\""));
        assert!(content.contains("\"kinds\":[\"search\"]"));
        assert!(content.contains("\"outcome\":\"ok\""));
        assert!(content.contains("\"local\":true"));

        let _ = std::fs::remove_file(tmp_egress);
    }

    #[tokio::test]
    async fn chain_order_ddg_202_falls_through_to_wikipedia() {
        let searxng_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let brave_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ddg_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wiki_contacted = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // SearXNG and Brave are unconfigured
        let searxng = Arc::new(MockBackend {
            name: "SearXNG",
            provider_id: "searxng",
            configured: false,
            is_local: false,
            response: Err("unconfigured".to_string()),
            contacted: searxng_contacted.clone(),
        });
        let brave = Arc::new(MockBackend {
            name: "Brave",
            provider_id: "brave",
            configured: false,
            is_local: false,
            response: Err("unconfigured".to_string()),
            contacted: brave_contacted.clone(),
        });
        // DuckDuckGo answers with HTTP 202 anti-bot failure
        let ddg = Arc::new(MockBackend {
            name: "DuckDuckGo",
            provider_id: "duckduckgo",
            configured: true,
            is_local: false,
            response: Err("duckduckgo returned HTTP 202".to_string()),
            contacted: ddg_contacted.clone(),
        });
        // Wikipedia succeeds
        let wiki = Arc::new(MockBackend {
            name: "Wikipedia",
            provider_id: "wikipedia",
            configured: true,
            is_local: false,
            response: Ok(vec![SearchResult {
                title: "Rust (programming language)".to_string(),
                url: "https://en.wikipedia.org/wiki/Rust_(programming_language)".to_string(),
                snippet: "Rust is a general-purpose programming language.".to_string(),
            }]),
            contacted: wiki_contacted.clone(),
        });

        let tmp_egress = std::env::temp_dir().join(format!(
            "test_egress_fallthrough_{}.jsonl",
            uuid::Uuid::new_v4().simple()
        ));
        let searcher = ChainedWebSearcher::with_backends(vec![searxng, brave, ddg, wiki])
            .with_egress_log_path(tmp_egress.clone());

        let res = searcher
            .search("who maintains rust")
            .await
            .expect("wikipedia succeeds");
        assert_eq!(res.backend, "Wikipedia");
        assert_eq!(res.results[0].title, "Rust (programming language)");

        assert!(!searxng_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!brave_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(ddg_contacted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(wiki_contacted.load(std::sync::atomic::Ordering::SeqCst));

        // Check egress log entries: one for DDG (error), one for Wikipedia (ok)
        let content = std::fs::read_to_string(&tmp_egress).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "must have 2 egress entries: {content}");
        assert!(lines[0].contains("\"provider\":\"duckduckgo\""));
        assert!(lines[0].contains("\"outcome\":\"error\""));
        assert!(lines[1].contains("\"provider\":\"wikipedia\""));
        assert!(lines[1].contains("\"outcome\":\"ok\""));

        let _ = std::fs::remove_file(tmp_egress);
    }

    #[tokio::test]
    async fn keys_never_appear_in_errors_or_logs() {
        let secret_key = "super-secret-brave-api-token-xyz-123456789";
        // Point Brave backend to an endpoint returning an error or invalid JSON
        let brave = BraveBackend::with_endpoint(
            Some(secret_key.to_string()),
            "http://127.0.0.1:9/invalid-will-fail",
        );

        let tmp_egress = std::env::temp_dir().join(format!(
            "test_egress_secret_{}.jsonl",
            uuid::Uuid::new_v4().simple()
        ));
        let searcher = ChainedWebSearcher::with_backends(vec![Arc::new(brave)])
            .with_egress_log_path(tmp_egress.clone());

        let res = searcher.search("test secret query").await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err();

        // The returned error string MUST NOT contain the secret key
        assert!(
            !err_msg.contains(secret_key),
            "error message contained secret key: {err_msg}"
        );

        // The egress log file MUST NOT contain the secret key
        if tmp_egress.exists() {
            let content = std::fs::read_to_string(&tmp_egress).unwrap();
            assert!(
                !content.contains(secret_key),
                "egress.jsonl contained secret key: {content}"
            );
            let _ = std::fs::remove_file(tmp_egress);
        }
    }

    #[test]
    fn appends_sources_names_backend_and_cites() {
        let results = vec![
            SearchResult {
                title: "Rust Doc".to_string(),
                url: "https://example.com/rust".to_string(),
                snippet: "Snippet 1".to_string(),
            },
            SearchResult {
                title: "Rust Wiki".to_string(),
                url: "https://en.wikipedia.org/wiki/Rust".to_string(),
                snippet: "Snippet 2".to_string(),
            },
        ];

        let answer = "According to [1], Rust is fast.";
        let with_sources = append_sources(answer, "Wikipedia", &results);
        assert!(with_sources.starts_with(answer));
        assert!(
            with_sources.contains("Sources (Wikipedia):\n[1] Rust Doc — https://example.com/rust")
        );
        assert!(!with_sources.contains("[2]"));
    }

    #[test]
    fn live_questions_search_the_web_without_being_told() {
        for q in [
            "How is the weather today in Chennai?",
            "will it rain tomorrow in Pune",
            "latest news on the rupee",
            "what's the score in the India match today",
            "price of gold",
            "USD to INR exchange rate",
            "who won the election yesterday",
            "Who won Wimbledon?",
            "Is it raining in Pune right now?",
            "What's my local weather today?",
            "temperature in Delhi",
        ] {
            assert!(super::wants_live(q), "{q}");
        }
        for q in [
            "What does the Rust error E0308 mean?",
            "what's my cpu temperature",
            "how is my battery today",
            "summarise this file",
            "explain the latest commit in this folder",
            "write a poem about rain",
            "How do RSS news feeds work?",
            "What oven temperature do I need for sourdough?",
            "Summarize the latest news in this article",
        ] {
            assert!(!super::wants_live(q), "{q}");
        }
    }
}
