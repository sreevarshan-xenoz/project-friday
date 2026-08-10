//! WebAgent layer (`specs/browser-organ.md` §3) — site adapters over the
//! browser verbs.
//!
//! FRIDAY's cognitive core never needs to know that ChatGPT is "ChatGPT". Site
//! behavior lives behind the [`WebAgent`] trait; each implementation owns a
//! small list of CSS-selector candidates per concern (prompt / send / response)
//! and degrades from specific to generic. Adapters are **best-effort by
//! design** — a site DOM change breaks a selector list, never silently the
//! whole organ. The LLM never drives the browser directly: hosts call
//! [`agent_for_site`] and talk to the trait.
//!
//! Adapters run only their own hard-coded JavaScript snippets through
//! `web.evaluate` — never caller-supplied strings.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::adapters::provider::{ProviderId, ProviderRegistry};
use crate::adapters::{ActionAdapter, BrowserAdapter};
use crate::request::{ActionRequest, ActionResult};

/// A reply from a web agent: the extracted text plus page identity.
#[derive(Debug, Clone)]
pub struct WebAgentResponse {
    pub text: String,
    pub title: String,
    pub url: String,
}

/// Polling budget for [`WebAgent::read_response`].
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// Generous budget for the generic adapter (no stop-button signal to rely on).
const GENERIC_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The one interface the cognitive core needs to talk to a logged-in website.
#[async_trait]
pub trait WebAgent: Send + Sync {
    /// Human-readable site name (for logs / routing / the audit trail).
    fn site(&self) -> &str;
    /// Open the site. The dedicated FRIDAY profile is already logged in.
    async fn open(&self, url: &str) -> Result<(), String>;
    /// Send a message to the current conversation/box.
    async fn send_message(&self, message: &str) -> Result<(), String>;
    /// Wait for the reply to finish generating and extract it.
    async fn read_response(&self) -> Result<WebAgentResponse, String>;
    /// Convenience: send, wait, read.
    async fn ask(&self, message: &str) -> Result<WebAgentResponse, String> {
        self.send_message(message).await?;
        self.read_response().await
    }
}

/// Route a site URL to the right site adapter. The mapping is keyed by the
/// canonical [`ProviderId`] (which owns the host rules): `chatgpt.com` →
/// [`ChatGPTAdapter`], `grok.com` → [`GrokAdapter`], `gemini.google.com` →
/// [`GeminiAdapter`], `claude.ai` → [`ClaudeAdapter`], `perplexity.ai` →
/// [`PerplexityAdapter`], everything else → [`GenericWebAgent`]. Lookalike
/// hosts never map: `x.com`'s tweet composer matches Grok's selectors but
/// stays generic (would type into a tweet box), and `gemini.com` (crypto
/// exchange) never routes to Google's AI product.
pub fn agent_for_site(url: &str, browser: BrowserAdapter) -> Box<dyn WebAgent> {
    match ProviderId::from_site(url) {
        ProviderId::ChatGPT => Box::new(ChatGPTAdapter::new(browser)),
        ProviderId::Grok => Box::new(GrokAdapter::new(browser)),
        ProviderId::Gemini => Box::new(GeminiAdapter::new(browser)),
        ProviderId::Claude => Box::new(ClaudeAdapter::new(browser)),
        ProviderId::Perplexity => Box::new(PerplexityAdapter::new(browser)),
        // Local/Generic have no dedicated site adapter — the generic adapter
        // is the safe, coarser fallback.
        _ => Box::new(GenericWebAgent::new(browser)),
    }
}

/// Shared helpers over the browser adapter's public verbs. Every interaction is
/// DOM-driven (CSS selectors) — never pixel coordinates.
struct BrowserSession<'a> {
    adapter: &'a BrowserAdapter,
}

impl<'a> BrowserSession<'a> {
    fn new(adapter: &'a BrowserAdapter) -> Self {
        Self { adapter }
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        let res = self
            .adapter
            .execute(ActionRequest::new("web", "open", json!({ "url": url })))
            .await;
        if res.success {
            Ok(())
        } else {
            Err(res.error.unwrap_or_else(|| "open failed".to_string()))
        }
    }

    /// Return `(matched_selector, text)` for the first candidate that matches.
    async fn probe(&self, candidates: &[&str]) -> Result<(String, String), String> {
        for sel in candidates {
            let res = self
                .adapter
                .execute(ActionRequest::new(
                    "web",
                    "extract",
                    json!({ "selector": sel }),
                ))
                .await;
            if res.success {
                let text = res
                    .output
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok((sel.to_string(), text));
            }
        }
        Err("none of the candidate selectors matched the current page".to_string())
    }

    /// True when any candidate selector currently matches.
    async fn exists(&self, candidates: &[&str]) -> bool {
        self.probe(candidates).await.is_ok()
    }

    /// Click the first candidate that matches.
    async fn click_first(&self, candidates: &[&str]) -> Result<(), String> {
        let (sel, _) = self.probe(candidates).await?;
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "web",
                "click",
                json!({ "selector": sel }),
            ))
            .await;
        if res.success {
            Ok(())
        } else {
            Err(res.error.unwrap_or_else(|| "click failed".to_string()))
        }
    }

    /// Press Enter on the first candidate that matches (form submission).
    async fn submit_first(&self, candidates: &[&str]) -> Result<(), String> {
        let (sel, _) = self.probe(candidates).await?;
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "web",
                "submit",
                json!({ "selector": sel }),
            ))
            .await;
        if res.success {
            Ok(())
        } else {
            Err(res.error.unwrap_or_else(|| "submit failed".to_string()))
        }
    }

    /// Run a fixed JS snippet (adapters never run caller-supplied JS) and
    /// return the string the expression evaluated to.
    async fn eval_text(&self, expression: &str) -> Result<String, String> {
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "web",
                "evaluate",
                json!({ "expression": expression }),
            ))
            .await;
        if res.success {
            Ok(res
                .output
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string())
        } else {
            Err(res.error.unwrap_or_else(|| "evaluate failed".to_string()))
        }
    }

    /// The inner text of the **last** element matching `selector` (streaming
    /// responses grow the last node, so we want the tail, not the head).
    async fn last_matching_text(&self, selector: &str) -> Result<String, String> {
        let js = format!(
            "(() => {{ const els = document.querySelectorAll({sel:?}); \
             const el = els[els.length - 1]; return el ? el.innerText : ''; }})()",
            sel = selector
        );
        self.eval_text(&js).await
    }

    /// First non-empty text among the candidate selectors, else an error.
    async fn first_nonempty_text(&self, selectors: &[&str]) -> Result<String, String> {
        for sel in selectors {
            let text = self.last_matching_text(sel).await.unwrap_or_default();
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
        Err("no response text found on the page".to_string())
    }

    async fn title(&self) -> String {
        self.eval_text("document.title").await.unwrap_or_default()
    }

    async fn current_url(&self) -> String {
        self.eval_text("location.href").await.unwrap_or_default()
    }
}

/// ChatGPT web adapter (`chatgpt.com`). Selector candidates are a living list —
/// ChatGPT changes its DOM; when a candidate stops matching, update the list.
const PROMPT_SELECTORS: &[&str] = &[
    "#prompt-textarea",
    "div[contenteditable='true']",
    "textarea",
];
const SEND_SELECTORS: &[&str] = &[
    "button[data-testid='send-button']",
    "button[data-testid='fruitjuice-send-button']",
    "form button[type='submit']",
];
const RESPONSE_SELECTOR: &str = "[data-message-author-role='assistant']";
const STOP_SELECTORS: &[&str] = &[
    "button[data-testid='stop-button']",
    "button[data-testid='fruitjuice-stop-button']",
    "button[aria-label*='Stop']",
];

pub struct ChatGPTAdapter {
    browser: BrowserAdapter,
    /// Assistant text observed before the current request was sent. The next
    /// reply must differ from this baseline (multi-turn safe).
    baseline: tokio::sync::Mutex<String>,
}

impl ChatGPTAdapter {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for ChatGPTAdapter {
    fn site(&self) -> &str {
        "chatgpt.com"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(PROMPT_SELECTORS).await.map_err(|_| {
            "no ChatGPT prompt box found — is chatgpt.com open and logged in? \
             (or ChatGPT changed its DOM — update the selector list)"
                .to_string()
        })?;
        // Record the baseline so read_response waits for a NEW reply.
        let baseline = s
            .last_matching_text(RESPONSE_SELECTOR)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(SEND_SELECTORS).await.is_err() {
            // No send button found — fall back to pressing Enter on the box.
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let generating = s.exists(STOP_SELECTORS).await;
            let text = s
                .last_matching_text(RESPONSE_SELECTOR)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev && (stable >= 2 || !generating) {
                    return Ok(WebAgentResponse { text, title, url });
                }
                prev = text;
                stable += 1;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for ChatGPT to finish responding".to_string())
    }
}

/// Grok web adapter (`grok.com`). Selector candidates are a living list —
/// Grok changes its DOM; when a candidate stops matching, update the list.
const GROK_PROMPT_SELECTORS: &[&str] = &[
    "textarea[placeholder*='Ask']",
    "div[contenteditable='true']",
    "textarea",
];
const GROK_SEND_SELECTORS: &[&str] = &[
    "button[aria-label*='Send']",
    "button[data-testid*='send']",
    "button[type='submit']",
];
const GROK_RESPONSE_SELECTORS: &[&str] =
    &["[data-testid*='message']", "[class*='message']", "article"];
const GROK_STOP_SELECTORS: &[&str] = &[
    "button[aria-label*='Stop']",
    "button[data-testid*='stop']",
    "button[aria-label*='Stop generating']",
];

pub struct GrokAdapter {
    browser: BrowserAdapter,
    baseline: tokio::sync::Mutex<String>,
}

impl GrokAdapter {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for GrokAdapter {
    fn site(&self) -> &str {
        "grok.com"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(GROK_PROMPT_SELECTORS).await.map_err(|_| {
            "no Grok prompt box found — is grok.com open and logged in? \
             (or Grok changed its DOM — update the selector list)"
                .to_string()
        })?;
        let baseline = s
            .first_nonempty_text(GROK_RESPONSE_SELECTORS)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(GROK_SEND_SELECTORS).await.is_err() {
            // No send button found — fall back to pressing Enter on the box.
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let generating = s.exists(GROK_STOP_SELECTORS).await;
            let text = s
                .first_nonempty_text(GROK_RESPONSE_SELECTORS)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev && (stable >= 2 || !generating) {
                    return Ok(WebAgentResponse { text, title, url });
                }
                prev = text;
                stable += 1;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for Grok to finish responding".to_string())
    }
}

/// Gemini web adapter (`gemini.google.com`). Selector candidates are a living
/// list — Gemini changes its DOM; when a candidate stops matching, update the
/// list.
const GEMINI_PROMPT_SELECTORS: &[&str] =
    &["rich-textarea", "div[contenteditable='true']", "textarea"];
const GEMINI_SEND_SELECTORS: &[&str] = &[
    "button[aria-label*='Send']",
    "button.send-button",
    "button[type='submit']",
];
const GEMINI_RESPONSE_SELECTORS: &[&str] =
    &[".model-response-text", "[class*='response']", "article"];
const GEMINI_STOP_SELECTORS: &[&str] = &[
    "button[aria-label*='Stop']",
    "button[data-testid*='stop']",
    "button[aria-label*='Stop generating']",
];

pub struct GeminiAdapter {
    browser: BrowserAdapter,
    baseline: tokio::sync::Mutex<String>,
}

impl GeminiAdapter {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for GeminiAdapter {
    fn site(&self) -> &str {
        "gemini.google.com"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(GEMINI_PROMPT_SELECTORS).await.map_err(|_| {
            "no Gemini prompt box found — is gemini.google.com open and logged in? \
             (or Gemini changed its DOM — update the selector list)"
                .to_string()
        })?;
        let baseline = s
            .first_nonempty_text(GEMINI_RESPONSE_SELECTORS)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(GEMINI_SEND_SELECTORS).await.is_err() {
            // No send button found — fall back to pressing Enter on the box.
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let generating = s.exists(GEMINI_STOP_SELECTORS).await;
            let text = s
                .first_nonempty_text(GEMINI_RESPONSE_SELECTORS)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev && (stable >= 2 || !generating) {
                    return Ok(WebAgentResponse { text, title, url });
                }
                prev = text;
                stable += 1;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for Gemini to finish responding".to_string())
    }
}

/// Claude web adapter (`claude.ai`). Selector candidates are a living list —
/// Claude changes its DOM; when a candidate stops matching, update the list.
const CLAUDE_PROMPT_SELECTORS: &[&str] = &[
    // Claude's composer is a ProseMirror editor div.
    "div.ProseMirror[contenteditable='true']",
    "div[contenteditable='true']",
    "textarea",
];
const CLAUDE_SEND_SELECTORS: &[&str] = &[
    "button[aria-label*='Send']",
    "button[aria-label*='submit']",
    "button[data-testid*='send']",
    "button[type='submit']",
];
const CLAUDE_RESPONSE_SELECTORS: &[&str] = &[
    ".font-claude-message",
    "[data-testid*='message']",
    "[class*='message']",
    "article",
];
const CLAUDE_STOP_SELECTORS: &[&str] = &[
    "button[aria-label*='Stop']",
    "button[data-testid*='stop']",
    "button[aria-label*='Stop generating']",
];

pub struct ClaudeAdapter {
    browser: BrowserAdapter,
    baseline: tokio::sync::Mutex<String>,
}

impl ClaudeAdapter {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for ClaudeAdapter {
    fn site(&self) -> &str {
        "claude.ai"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(CLAUDE_PROMPT_SELECTORS).await.map_err(|_| {
            "no Claude prompt box found — is claude.ai open and logged in? \
             (or Claude changed its DOM — update the selector list)"
                .to_string()
        })?;
        let baseline = s
            .first_nonempty_text(CLAUDE_RESPONSE_SELECTORS)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(CLAUDE_SEND_SELECTORS).await.is_err() {
            // No send button found — fall back to pressing Enter on the box.
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let generating = s.exists(CLAUDE_STOP_SELECTORS).await;
            let text = s
                .first_nonempty_text(CLAUDE_RESPONSE_SELECTORS)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev && (stable >= 2 || !generating) {
                    return Ok(WebAgentResponse { text, title, url });
                }
                prev = text;
                stable += 1;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for Claude to finish responding".to_string())
    }
}

/// Perplexity web adapter (`perplexity.ai`). Selector candidates are a living
/// list — Perplexity changes its DOM; when a candidate stops matching, update
/// the list.
const PERPLEXITY_PROMPT_SELECTORS: &[&str] = &[
    "textarea[placeholder*='Ask']",
    "textarea[placeholder*='Search']",
    "div[contenteditable='true']",
    "textarea",
];
const PERPLEXITY_SEND_SELECTORS: &[&str] = &[
    "button[aria-label*='Submit']",
    "button[aria-label*='Send']",
    "button[type='submit']",
];
const PERPLEXITY_RESPONSE_SELECTORS: &[&str] = &[
    "[class*='answer']",
    "[class*='response']",
    "div.prose",
    "article",
];
const PERPLEXITY_STOP_SELECTORS: &[&str] = &[
    "button[aria-label*='Stop']",
    "button[data-testid*='stop']",
    "button[aria-label*='Stop generating']",
];

pub struct PerplexityAdapter {
    browser: BrowserAdapter,
    baseline: tokio::sync::Mutex<String>,
}

impl PerplexityAdapter {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for PerplexityAdapter {
    fn site(&self) -> &str {
        "perplexity.ai"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(PERPLEXITY_PROMPT_SELECTORS).await.map_err(|_| {
            "no Perplexity prompt box found — is perplexity.ai open and logged in? \
             (or Perplexity changed its DOM — update the selector list)"
                .to_string()
        })?;
        let baseline = s
            .first_nonempty_text(PERPLEXITY_RESPONSE_SELECTORS)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(PERPLEXITY_SEND_SELECTORS).await.is_err() {
            // No send button found — fall back to pressing Enter on the box.
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let generating = s.exists(PERPLEXITY_STOP_SELECTORS).await;
            let text = s
                .first_nonempty_text(PERPLEXITY_RESPONSE_SELECTORS)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev && (stable >= 2 || !generating) {
                    return Ok(WebAgentResponse { text, title, url });
                }
                prev = text;
                stable += 1;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for Perplexity to finish responding".to_string())
    }
}

/// Generic adapter: any site with a text box and a submit affordance. Slower
/// and coarser than a site-specific adapter, but works everywhere.
const GENERIC_PROMPT_SELECTORS: &[&str] = &[
    "textarea",
    "[contenteditable='true']",
    "input[type='text']",
    "input[type='search']",
];
const GENERIC_SEND_SELECTORS: &[&str] = &[
    "button[type='submit']",
    "input[type='submit']",
    "form button",
];
const GENERIC_RESPONSE_SELECTORS: &[&str] = &[
    "article",
    "[class*='response']",
    "[class*='answer']",
    "[class*='result']",
];

pub struct GenericWebAgent {
    browser: BrowserAdapter,
    baseline: tokio::sync::Mutex<String>,
}

impl GenericWebAgent {
    pub fn new(browser: BrowserAdapter) -> Self {
        Self {
            browser,
            baseline: tokio::sync::Mutex::new(String::new()),
        }
    }
}

#[async_trait]
impl WebAgent for GenericWebAgent {
    fn site(&self) -> &str {
        "generic-web"
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        BrowserSession::new(&self.browser).open(url).await
    }

    async fn send_message(&self, message: &str) -> Result<(), String> {
        let s = BrowserSession::new(&self.browser);
        let (prompt_sel, _) = s.probe(GENERIC_PROMPT_SELECTORS).await.map_err(|_| {
            "no text box found on this page (the generic adapter needs a textarea, \
             contenteditable, or text input)"
                .to_string()
        })?;
        let baseline = s
            .first_nonempty_text(GENERIC_RESPONSE_SELECTORS)
            .await
            .unwrap_or_default();
        *self.baseline.lock().await = baseline;

        let res = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "type",
                json!({ "selector": prompt_sel, "text": message }),
            ))
            .await;
        if !res.success {
            return Err(res.error.unwrap_or_else(|| "type failed".to_string()));
        }
        if s.click_first(GENERIC_SEND_SELECTORS).await.is_err() {
            s.submit_first(&[&prompt_sel]).await?;
        }
        Ok(())
    }

    async fn read_response(&self) -> Result<WebAgentResponse, String> {
        let s = BrowserSession::new(&self.browser);
        let baseline = self.baseline.lock().await.clone();
        let title = s.title().await;
        let url = s.current_url().await;
        let mut prev = String::new();
        let mut stable = 0u32;
        let deadline = std::time::Instant::now() + GENERIC_TIMEOUT;
        while std::time::Instant::now() < deadline {
            let text = s
                .first_nonempty_text(GENERIC_RESPONSE_SELECTORS)
                .await
                .unwrap_or_default();
            let is_new = !text.trim().is_empty() && text != baseline;
            if is_new {
                if text == prev {
                    stable += 1;
                    if stable >= 2 {
                        return Ok(WebAgentResponse { text, title, url });
                    }
                } else {
                    stable = 0;
                }
                prev = text;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Err("timed out waiting for the page to produce a response".to_string())
    }
}

/// Environment override for the dedicated browser profile (default
/// `~/.friday/browser/` — the Browser Organ's credential boundary).
pub(crate) const PROFILE_ENV: &str = "FRIDAY_BROWSER_PROFILE";
/// Environment override for the default site asked when a request carries no
/// explicit `site` (default `https://chatgpt.com`).
const DEFAULT_SITE_ENV: &str = "FRIDAY_WEBAGENT_SITE";
/// Environment override for the default **provider identity** when a request
/// carries no explicit `provider`/`site` (e.g. `FRIDAY_WEBAGENT_PROVIDER=grok`).
/// Resolves through [`ProviderId::from_name`] to the identity's canonical URL,
/// so identity and address can never drift.
const DEFAULT_PROVIDER_ENV: &str = "FRIDAY_WEBAGENT_PROVIDER";

/// Resolve the dedicated browser profile: `FRIDAY_BROWSER_PROFILE` env
/// override, else `~/.friday/browser/`. Shared by the WebAgent adapter and the
/// ReasoningProvider layer (one source of truth for the credential boundary).
pub(crate) fn browser_profile_dir() -> std::path::PathBuf {
    std::env::var(PROFILE_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| default_profile_dir())
}

/// Default dedicated profile directory: `~/.friday/browser/`.
fn default_profile_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".friday")
        .join("browser")
}

/// Normalize a site param: a bare host (`chatgpt.com`) becomes a full URL.
fn normalize_site(site: &str) -> String {
    let t = site.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    }
}

/// Session-pool key. For a known provider it is the **identity** —
/// `chatgpt.com` and `chat.openai.com` are one ChatGPT session (URLs change;
/// identity doesn't). For generic sites it is the canonical host+path (two
/// generic sites stay distinct sessions). Trailing slash/query/fragment are
/// stripped in both cases, so `https://chatgpt.com/?x=1` and `chatgpt.com/`
/// share the ChatGPT session and `ask`-then-`read` never relaunches the
/// browser.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SessionKey {
    Provider(ProviderId),
    Host(String),
}

fn session_key(site: &str) -> SessionKey {
    let id = ProviderId::from_site(site);
    if id.is_known() {
        return SessionKey::Provider(id);
    }
    let mut s = normalize_site(site);
    if let Some(rest) = s.split_once("://") {
        s = rest.1.to_string();
    }
    for sep in ['?', '#'] {
        if let Some(rest) = s.split_once(sep) {
            s = rest.0.to_string();
        }
    }
    SessionKey::Host(s.trim_end_matches('/').to_string())
}

/// Build a fresh [`WebAgent`] for a site over the dedicated profile. The
/// default factory routes through [`agent_for_site`]; tests inject a fake so
/// the session-pool semantics can be asserted hermetically (no browser).
type WebAgentFactory = Arc<dyn Fn(&str, std::path::PathBuf) -> Box<dyn WebAgent> + Send + Sync>;

/// The planner-visible adapter over the WebAgent layer.
///
/// Exposes the Browser Organ to goal-execution as `webagent.ask / send / read`
/// — an evidence action `gather evidence: external (webagent.ask)` routes here
/// and runs against the dedicated, logged-in browser profile.
///
/// **Planner-path defaults** (so the evidence seam needs zero extra plumbing):
/// - `site` defaults to `FRIDAY_WEBAGENT_SITE` (else `https://chatgpt.com`);
/// - `message` falls back to the `query` param — which the planner always sets
///   to the goal description for evidence actions.
///
/// **Session pool**: one live browser session per expert is kept alive across
/// actions, keyed by [`SessionKey`] — the **provider identity** for known
/// providers (`chatgpt.com` and `chat.openai.com` are one ChatGPT session),
/// the canonical host+path for generic sites. Asking ChatGPT, then Grok, then
/// ChatGPT again reuses both sessions instead of dismissing one expert and
/// rebuilding it on return — each new site launches on demand against the same
/// on-disk profile (still logged in).
pub struct WebAgentAdapter {
    profile: std::path::PathBuf,
    default_site: String,
    sessions: tokio::sync::Mutex<HashMap<SessionKey, Box<dyn WebAgent>>>,
    factory: WebAgentFactory,
    /// The identity → {url, profile} mapping the `provider` param resolves
    /// through. Deployments customize addresses here; the default is the
    /// shipped identity set. One place a provider's URL lives.
    registry: ProviderRegistry,
}

impl Default for WebAgentAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebAgentAdapter {
    pub fn new() -> Self {
        let profile = browser_profile_dir();
        // `FRIDAY_WEBAGENT_PROVIDER` (identity) wins over the legacy site URL
        // var; the identity's canonical URL becomes the default site.
        let default_site = std::env::var(DEFAULT_PROVIDER_ENV)
            .ok()
            .and_then(|n| ProviderId::from_name(&n).map(|id| id.default_url().to_string()))
            .or_else(|| std::env::var(DEFAULT_SITE_ENV).ok())
            .unwrap_or_else(|| ProviderId::ChatGPT.default_url().to_string());
        Self {
            profile,
            default_site,
            sessions: tokio::sync::Mutex::new(HashMap::new()),
            factory: Arc::new(|site: &str, profile: std::path::PathBuf| {
                agent_for_site(site, BrowserAdapter::new().with_user_data_dir(profile))
            }),
            registry: ProviderRegistry::default_browser(),
        }
    }

    /// Override the dedicated browser profile directory.
    pub fn with_profile(mut self, profile: std::path::PathBuf) -> Self {
        self.profile = profile;
        self
    }

    /// Override the default site used when a request carries no `site`.
    pub fn with_default_site(mut self, site: impl Into<String>) -> Self {
        self.default_site = normalize_site(&site.into());
        self
    }

    /// Override how a fresh agent is built for a site. The default routes
    /// through [`agent_for_site`] over the dedicated profile; tests inject a
    /// fake factory to assert pool semantics hermetically.
    ///
    /// The factory runs **while the pool lock is held** (tokio Mutex, not
    /// reentrant) — it must be fast and must not re-enter this adapter.
    pub fn with_agent_factory(mut self, factory: WebAgentFactory) -> Self {
        self.factory = factory;
        self
    }

    /// Replace the provider registry the `provider` param resolves through.
    /// Deployments re-point an identity's URL here (e.g. Grok behind a proxy);
    /// the default is [`ProviderRegistry::default_browser`].
    pub fn with_registry(mut self, registry: ProviderRegistry) -> Self {
        self.registry = registry;
        self
    }

    fn resolve_site(&self, req: &ActionRequest) -> String {
        let site = req
            .parameters
            .get("site")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());
        normalize_site(site.unwrap_or(&self.default_site))
    }

    /// Resolve which expert a request targets, returning the (identity, URL)
    /// pair. An explicit `provider` param (canonical identity, e.g. `"grok"`)
    /// wins and resolves through **the registry** — a deployment-customized
    /// address takes effect here; else the `site` param; else the default
    /// site. `generic` has no canonical address, so it falls through to site
    /// resolution (the site/default IS the identity for a generic site).
    fn resolve_target(&self, req: &ActionRequest) -> (ProviderId, String) {
        if let Some(name) = req.parameters.get("provider").and_then(|v| v.as_str()) {
            if let Some(id) = ProviderId::from_name(name) {
                if !matches!(id, ProviderId::Generic) {
                    let url = self
                        .registry
                        .url(id)
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| id.default_url().to_string());
                    return (id, url);
                }
            }
        }
        let site = self.resolve_site(req);
        (ProviderId::from_site(&site), site)
    }

    fn resolve_message(&self, req: &ActionRequest) -> Option<String> {
        let m = req
            .parameters
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| req.parameters.get("query").and_then(|v| v.as_str()));
        m.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Reuse the pooled session for `site` or build + open a fresh agent.
    /// Sessions are keyed by identity for known providers (chatgpt.com and
    /// chat.openai.com share one ChatGPT session) and by canonical host+path
    /// for generic sites — and returning to a previously asked expert reuses
    /// its live session instead of rebuilding it.
    ///
    /// One pool-wide lock guards all sessions: concurrent asks to *different*
    /// experts serialize on it (behaviorally unchanged from the single-session
    /// design). Per-session locks are a future optimization if multi-provider
    /// fan-out ever needs them.
    async fn ensure_agent<'a>(
        &'a self,
        site: &str,
    ) -> Result<tokio::sync::MutexGuard<'a, HashMap<SessionKey, Box<dyn WebAgent>>>, String> {
        let key = session_key(site);
        let mut sessions = self.sessions.lock().await;
        if !sessions.contains_key(&key) {
            let agent = (self.factory)(site, self.profile.clone());
            agent
                .open(site)
                .await
                .map_err(|e| format!("webagent open {site}: {e}"))?;
            sessions.insert(key.clone(), agent);
        }
        Ok(sessions)
    }

    async fn ask(&self, req: &ActionRequest) -> ActionResult {
        let (provider, site) = self.resolve_target(req);
        let Some(message) = self.resolve_message(req) else {
            return ActionResult::error(
                "webagent.ask requires a message (param 'message' or 'query')",
            );
        };
        let sessions = match self.ensure_agent(&site).await {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let agent = sessions.get(&session_key(&site)).expect("session pooled");
        match agent.ask(&message).await {
            Ok(r) => ActionResult::success(json!({
                "text": r.text, "title": r.title, "url": r.url, "site": site,
                "provider": provider.name(),
            })),
            Err(e) => ActionResult::error(e),
        }
    }

    async fn send(&self, req: &ActionRequest) -> ActionResult {
        let (provider, site) = self.resolve_target(req);
        let Some(message) = self.resolve_message(req) else {
            return ActionResult::error(
                "webagent.send requires a message (param 'message' or 'query')",
            );
        };
        let sessions = match self.ensure_agent(&site).await {
            Ok(s) => s,
            Err(e) => return ActionResult::error(e),
        };
        let agent = sessions.get(&session_key(&site)).expect("session pooled");
        match agent.send_message(&message).await {
            Ok(()) => ActionResult::success(
                json!({ "sent": true, "site": site, "provider": provider.name() }),
            ),
            Err(e) => ActionResult::error(e),
        }
    }

    async fn read(&self, req: &ActionRequest) -> ActionResult {
        let (provider, site) = self.resolve_target(req);
        // Read never launches a browser: a standalone read on a cold session
        // has nothing to read. Mirror the browser adapter's contract
        // (`web.extract` errors "no browser — call web.open first").
        let key = session_key(&site);
        let sessions = self.sessions.lock().await;
        let Some(agent) = sessions.get(&key) else {
            return ActionResult::error(format!(
                "no webagent session open for {site} — call webagent.ask or webagent.send first"
            ));
        };
        match agent.read_response().await {
            Ok(r) => ActionResult::success(json!({
                "text": r.text, "title": r.title, "url": r.url, "site": site,
                "provider": provider.name(),
            })),
            Err(e) => ActionResult::error(e),
        }
    }
}

#[async_trait]
impl ActionAdapter for WebAgentAdapter {
    fn tool(&self) -> &str {
        "webagent"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["ask", "send", "read"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "ask" => self.ask(&req).await,
            "send" => self.send(&req).await,
            "read" => self.read(&req).await,
            other => ActionResult::error(format!("unknown webagent action: {other}")),
        }
    }
}

/// Capability records for the webagent verbs, for registration into the
/// planner's [`CapabilityRegistry`] so research goals can route through
/// logged-in sites. `ask`/`send` mutate a live page (`Risk::Medium`, like
/// `web.type`); `read` only extracts (`Risk::Safe`).
pub fn webagent_capability_specs(
) -> Vec<friday_planner::execution::capability_registry::CapabilitySpec> {
    use friday_planner::execution::capability_registry::{CapabilitySpec, Risk};
    vec![
        CapabilitySpec::native(
            "webagent.ask".to_string(),
            "Ask a logged-in website".to_string(),
            "Send a message to a logged-in website (e.g. chatgpt.com) through the dedicated \
             browser profile and read the reply. Params: provider (identity override, e.g. \
             \"grok\"), site (default FRIDAY_WEBAGENT_SITE), message (default: the goal)"
                .to_string(),
            "webagent".to_string(),
            "ask".to_string(),
            Risk::Medium,
        ),
        CapabilitySpec::native(
            "webagent.send".to_string(),
            "Send a message to a site".to_string(),
            "Send a message to a logged-in website without waiting for the reply. \
             Params: provider (identity override), site, message (default: the goal)"
                .to_string(),
            "webagent".to_string(),
            "send".to_string(),
            Risk::Medium,
        ),
        CapabilitySpec::native(
            "webagent.read".to_string(),
            "Read a site's reply".to_string(),
            "Read the latest reply from the open logged-in site. Params: site".to_string(),
            "webagent".to_string(),
            "read".to_string(),
            Risk::Safe,
        ),
    ]
}

/// Register the webagent verbs into the planner's capability registry. Call
/// alongside registering a [`WebAgentAdapter`] with the kernel/evidence seam.
pub fn register_webagent_capabilities(
    registry: &friday_planner::execution::capability_registry::CapabilityRegistry,
) {
    for spec in webagent_capability_specs() {
        registry.register(spec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_chatgpt_hosts_to_chatgpt_adapter() {
        let agent = agent_for_site("https://chatgpt.com", BrowserAdapter::new());
        assert_eq!(agent.site(), "chatgpt.com");
        let agent = agent_for_site("https://chat.openai.com/", BrowserAdapter::new());
        assert_eq!(agent.site(), "chatgpt.com");
    }

    #[test]
    fn routes_grok_hosts_to_grok_adapter() {
        let agent = agent_for_site("https://grok.com", BrowserAdapter::new());
        assert_eq!(agent.site(), "grok.com");
        let agent = agent_for_site("https://grok.com/chat", BrowserAdapter::new());
        assert_eq!(agent.site(), "grok.com");
    }

    #[test]
    fn routes_gemini_hosts_to_gemini_adapter() {
        let agent = agent_for_site("https://gemini.google.com", BrowserAdapter::new());
        assert_eq!(agent.site(), "gemini.google.com");
        let agent = agent_for_site("https://gemini.google.com/app", BrowserAdapter::new());
        assert_eq!(agent.site(), "gemini.google.com");
    }

    #[test]
    fn does_not_hijack_lookalike_hosts() {
        // x.com's tweet composer is a contenteditable that matches Grok's
        // selectors - it must NOT route to Grok (would type into the tweet box).
        let agent = agent_for_site("https://x.com/", BrowserAdapter::new());
        assert_eq!(agent.site(), "generic-web");
        // gemini.com is the crypto exchange, not Google's AI product.
        let agent = agent_for_site("https://gemini.com", BrowserAdapter::new());
        assert_eq!(agent.site(), "generic-web");
    }

    #[test]
    fn routes_claude_hosts_to_claude_adapter() {
        let agent = agent_for_site("https://claude.ai", BrowserAdapter::new());
        assert_eq!(agent.site(), "claude.ai");
        let agent = agent_for_site("https://claude.ai/new", BrowserAdapter::new());
        assert_eq!(agent.site(), "claude.ai");
    }

    #[test]
    fn routes_perplexity_hosts_to_perplexity_adapter() {
        let agent = agent_for_site("https://perplexity.ai", BrowserAdapter::new());
        assert_eq!(agent.site(), "perplexity.ai");
        let agent = agent_for_site("https://www.perplexity.ai/search", BrowserAdapter::new());
        assert_eq!(agent.site(), "perplexity.ai");
    }

    #[test]
    fn routes_other_hosts_to_generic_adapter() {
        let agent = agent_for_site("https://example.com", BrowserAdapter::new());
        assert_eq!(agent.site(), "generic-web");
        let agent = agent_for_site("https://someblog.example/posts", BrowserAdapter::new());
        assert_eq!(agent.site(), "generic-web");
    }

    // ── WebAgentAdapter (the planner-visible ActionAdapter) ───────────

    #[tokio::test]
    async fn adapter_unknown_action_errors() {
        let adapter = WebAgentAdapter::new();
        let res = adapter
            .execute(ActionRequest::new("webagent", "teleport", json!({})))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("unknown webagent action"));
    }

    /// Fake [`WebAgent`] for hermetic session-pool tests (no browser).
    struct FakeWebAgent {
        site: &'static str,
    }

    #[async_trait]
    impl WebAgent for FakeWebAgent {
        fn site(&self) -> &str {
            self.site
        }
        async fn open(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }
        async fn send_message(&self, _message: &str) -> Result<(), String> {
            Ok(())
        }
        async fn read_response(&self) -> Result<WebAgentResponse, String> {
            Ok(WebAgentResponse {
                text: format!("reply from {}", self.site),
                title: "fake".into(),
                url: "http://fake".into(),
            })
        }
    }

    /// The session pool keeps one live session per site: asking ChatGPT, then
    /// Grok, then ChatGPT again builds exactly two agents (the ChatGPT session
    /// is reused on return — no "dismiss the expert, invite it back" churn).
    #[tokio::test]
    async fn session_pool_reuses_agents_across_site_switches() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let builds = Arc::new(AtomicUsize::new(0));
        let builds_factory = Arc::clone(&builds);
        let adapter = WebAgentAdapter::new()
            .with_agent_factory(Arc::new(move |site: &str, _profile: std::path::PathBuf| {
                builds_factory.fetch_add(1, Ordering::SeqCst);
                let site = if site.contains("chatgpt") {
                    "chatgpt.com"
                } else if site.contains("grok") {
                    "grok.com"
                } else {
                    "generic-web"
                };
                Box::new(FakeWebAgent { site }) as Box<dyn WebAgent>
            }))
            .with_default_site("https://chatgpt.com");

        // 1. ChatGPT
        let r1 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "site": "https://chatgpt.com", "query": "first" }),
            ))
            .await;
        assert!(r1.success, "{r1:?}");
        // 2. Grok
        let r2 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "site": "https://grok.com", "query": "second" }),
            ))
            .await;
        assert!(r2.success, "{r2:?}");
        // 3. Back to ChatGPT — must reuse the pooled session, not rebuild.
        let r3 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "site": "https://chatgpt.com", "query": "third" }),
            ))
            .await;
        assert!(r3.success, "{r3:?}");
        assert_eq!(
            builds.load(Ordering::SeqCst),
            2,
            "returning to ChatGPT must reuse its pooled session (no rebuild)"
        );
        // Each reply carries the right site identity.
        assert_eq!(r1.output["text"], "reply from chatgpt.com");
        assert_eq!(r2.output["text"], "reply from grok.com");
        assert_eq!(r3.output["text"], "reply from chatgpt.com");

        // read on a cold site still refuses without launching a session.
        let cold = adapter
            .execute(ActionRequest::new(
                "webagent",
                "read",
                json!({ "site": "https://gemini.google.com" }),
            ))
            .await;
        assert!(!cold.success);
        assert!(
            cold.error.unwrap().contains("no webagent session open"),
            "cold read must refuse"
        );
        // No new session was built by the refused read.
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }

    /// Sessions are keyed by **provider identity**, not URL: `chatgpt.com`
    /// and `chat.openai.com` are the same ChatGPT expert, so switching hosts
    /// reuses the pooled session instead of relaunching the browser. This is
    /// the review-driven rule — URLs change; identity doesn't.
    #[tokio::test]
    async fn identity_merges_provider_hosts_into_one_session() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let builds = Arc::new(AtomicUsize::new(0));
        let builds_factory = Arc::clone(&builds);
        let adapter = WebAgentAdapter::new()
            .with_agent_factory(Arc::new(
                move |_site: &str, _profile: std::path::PathBuf| {
                    builds_factory.fetch_add(1, Ordering::SeqCst);
                    Box::new(FakeWebAgent {
                        site: "chatgpt.com",
                    }) as Box<dyn WebAgent>
                },
            ))
            .with_default_site("https://chatgpt.com");

        let r1 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "site": "https://chatgpt.com", "query": "first" }),
            ))
            .await;
        assert!(r1.success, "{r1:?}");
        let r2 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "site": "https://chat.openai.com/", "query": "second" }),
            ))
            .await;
        assert!(r2.success, "{r2:?}");
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "chat.openai.com is the same ChatGPT identity — no second session"
        );
        assert_eq!(r2.output["provider"], "chatgpt");
    }

    /// The `provider` param targets an expert by canonical identity: with a
    /// chatgpt default, `provider: "grok"` routes to grok.com, and the
    /// repeated call reuses the pooled Grok session (exactly one build).
    #[tokio::test]
    async fn provider_param_routes_to_identity() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let builds = Arc::new(AtomicUsize::new(0));
        let builds_factory = Arc::clone(&builds);
        let adapter = WebAgentAdapter::new()
            .with_agent_factory(Arc::new(move |site: &str, _profile: std::path::PathBuf| {
                builds_factory.fetch_add(1, Ordering::SeqCst);
                let site = if site.contains("grok") {
                    "grok.com"
                } else {
                    "chatgpt.com"
                };
                Box::new(FakeWebAgent { site }) as Box<dyn WebAgent>
            }))
            .with_default_site("https://chatgpt.com");

        let r1 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "provider": "grok", "query": "realtime?" }),
            ))
            .await;
        assert!(r1.success, "{r1:?}");
        assert_eq!(r1.output["provider"], "grok");
        assert_eq!(r1.output["site"], "https://grok.com");
        assert_eq!(r1.output["text"], "reply from grok.com");

        let r2 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "provider": "grok", "query": "again?" }),
            ))
            .await;
        assert!(r2.success, "{r2:?}");
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "identity-keyed pool reuse"
        );

        // An unknown provider name falls back to the default site (chatgpt).
        let r3 = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "provider": "not-a-provider", "query": "fallback?" }),
            ))
            .await;
        assert!(r3.success, "{r3:?}");
        assert_eq!(r3.output["provider"], "chatgpt");
    }

    /// A deployment-customized registry is what the `provider` param resolves
    /// through: re-pointing Grok at a proxy URL in the registry changes where
    /// the session opens — `insert()` is the live address source, not a
    /// doc-only view.
    #[tokio::test]
    async fn registry_customization_reroutes_provider_param() {
        use crate::adapters::provider::{ProviderEntry, Transport};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let builds = Arc::new(AtomicUsize::new(0));
        let builds_factory = Arc::clone(&builds);
        let mut registry = ProviderRegistry::default_browser();
        registry.insert(ProviderEntry {
            id: ProviderId::Grok,
            url: "https://grok.example.test".to_string(),
            transport: Transport::Browser,
            profile: ProviderId::Grok.default_profile(),
            capabilities: ProviderId::Grok.default_capabilities(),
        });
        let adapter = WebAgentAdapter::new()
            .with_registry(registry)
            .with_agent_factory(Arc::new(move |site: &str, _profile: std::path::PathBuf| {
                builds_factory.fetch_add(1, Ordering::SeqCst);
                let site = if site.contains("grok") {
                    "grok.com"
                } else {
                    "chatgpt.com"
                };
                Box::new(FakeWebAgent { site }) as Box<dyn WebAgent>
            }))
            .with_default_site("https://chatgpt.com");

        let r = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "provider": "grok", "query": "hi" }),
            ))
            .await;
        assert!(r.success, "{r:?}");
        assert_eq!(
            r.output["site"], "https://grok.example.test",
            "the registry address must win over the identity default"
        );
        assert_eq!(r.output["provider"], "grok");
    }

    /// `FRIDAY_WEBAGENT_PROVIDER` (identity) wins over the legacy site URL var
    /// and resolves to the identity's canonical address.
    ///
    /// This test mutates a process-wide env var while other tests run in
    /// parallel. Safe **only** because every other test that constructs
    /// `WebAgentAdapter::new()` either pins `with_default_site(...)` (overriding
    /// whatever `new()` read) or fails before site resolution — keep that
    /// invariant if you add tests that read the un-pinned default.
    #[test]
    fn provider_env_override_sets_default_site() {
        std::env::set_var(DEFAULT_PROVIDER_ENV, "gemini");
        let adapter = WebAgentAdapter::new();
        std::env::remove_var(DEFAULT_PROVIDER_ENV);
        assert_eq!(adapter.default_site, "https://gemini.google.com");
    }

    #[tokio::test]
    async fn adapter_ask_requires_a_message() {
        // No params at all: the site default applies, but the message must be
        // present — this must fail BEFORE any browser launches.
        let adapter = WebAgentAdapter::new();
        let res = adapter
            .execute(ActionRequest::new("webagent", "ask", json!({})))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("message"));
    }

    #[tokio::test]
    async fn adapter_query_param_is_used_as_the_message() {
        // The planner's evidence actions carry {"query": <goal>}; the adapter
        // must treat that as the message. Resolving the message is pure — no
        // browser needed to assert the fallback wiring. Pin the default site
        // explicitly so a developer's FRIDAY_WEBAGENT_SITE env var can't
        // change this hermetic assertion.
        let adapter = WebAgentAdapter::new().with_default_site("https://chatgpt.com");
        let req = ActionRequest::new(
            "webagent",
            "ask",
            json!({ "query": "Reply with exactly: OK" }),
        );
        assert_eq!(
            adapter.resolve_message(&req).as_deref(),
            Some("Reply with exactly: OK")
        );
        assert_eq!(adapter.resolve_site(&req), "https://chatgpt.com");
        // A trailing-slash/query form is the same session as the bare host.
        assert_eq!(
            session_key("https://chatgpt.com"),
            session_key("chatgpt.com/")
        );
        assert_eq!(
            session_key("https://chatgpt.com/?x=1"),
            session_key("https://chatgpt.com")
        );
    }

    /// Live browser integration: the adapter's `ask` runs the full WebAgent
    /// round trip against a local echo page (the planner-path defaults: no
    /// explicit site, message from `query`). Skipped unless
    /// `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn adapter_ask_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!(
                "skipping webagent adapter integration (set FRIDAY_BROWSER_TEST=1 to enable)"
            );
            return;
        }
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let adapter = WebAgentAdapter::new()
            .with_profile(std::env::temp_dir().join("friday-webagent-adapter-test"))
            .with_default_site(base);
        let res = adapter
            .execute(ActionRequest::new(
                "webagent",
                "ask",
                json!({ "query": "Hello adapter" }),
            ))
            .await;
        assert!(res.success, "ask failed: {:?}", res.error);
        assert!(
            res.output["text"]
                .as_str()
                .unwrap()
                .contains("Hello adapter"),
            "output: {}",
            res.output
        );
        let _ = server.abort();
    }

    /// Live browser integration for the generic adapter (headless Chrome
    /// required): open a local page, type into its textarea, submit via Enter,
    /// and read the echoed <article>. Skipped unless `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn generic_webagent_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!(
                "skipping webagent browser integration (set FRIDAY_BROWSER_TEST=1 to enable)"
            );
            return;
        }
        // Serialize with the browser adapter's live test: one headless Chrome
        // at a time.
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let agent = GenericWebAgent::new(BrowserAdapter::new());
        agent.open(&base).await.expect("open local page");
        agent
            .send_message("Hello FRIDAY echo")
            .await
            .expect("send message");
        let resp = agent.read_response().await.expect("read response");
        assert_eq!(resp.title, "Echo");
        assert!(
            resp.text.contains("Hello FRIDAY echo"),
            "got: {}",
            resp.text
        );
        let _ = server.abort();
    }

    /// Live browser integration for the Grok adapter: the local echo page's
    /// `textarea`/`article` match the Grok selector-list fallbacks, so the
    /// full open → type → Enter → read flow is proven against real Chrome.
    /// Skipped unless `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn grok_webagent_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping grok webagent integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let agent = GrokAdapter::new(BrowserAdapter::new());
        agent.open(&base).await.expect("open local page");
        agent
            .send_message("Hello Grok echo")
            .await
            .expect("send message");
        let resp = agent.read_response().await.expect("read response");
        assert_eq!(resp.title, "Echo");
        assert!(resp.text.contains("Hello Grok echo"), "got: {}", resp.text);
        let _ = server.abort();
    }

    /// Live browser integration for the Gemini adapter (same local echo page;
    /// `textarea`/`article` match the Gemini fallbacks). Skipped unless
    /// `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn gemini_webagent_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping gemini webagent integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let agent = GeminiAdapter::new(BrowserAdapter::new());
        agent.open(&base).await.expect("open local page");
        agent
            .send_message("Hello Gemini echo")
            .await
            .expect("send message");
        let resp = agent.read_response().await.expect("read response");
        assert_eq!(resp.title, "Echo");
        assert!(
            resp.text.contains("Hello Gemini echo"),
            "got: {}",
            resp.text
        );
        let _ = server.abort();
    }

    /// Live browser integration for the Claude adapter (same local echo page;
    /// `textarea`/`article` match the Claude fallbacks). Skipped unless
    /// `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn claude_webagent_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!("skipping claude webagent integration (set FRIDAY_BROWSER_TEST=1 to enable)");
            return;
        }
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let agent = ClaudeAdapter::new(BrowserAdapter::new());
        agent.open(&base).await.expect("open local page");
        agent
            .send_message("Hello Claude echo")
            .await
            .expect("send message");
        let resp = agent.read_response().await.expect("read response");
        assert_eq!(resp.title, "Echo");
        assert!(
            resp.text.contains("Hello Claude echo"),
            "got: {}",
            resp.text
        );
        let _ = server.abort();
    }

    /// Live browser integration for the Perplexity adapter (same local echo
    /// page; `textarea`/`article` match the Perplexity fallbacks). Skipped
    /// unless `FRIDAY_BROWSER_TEST=1`.
    #[tokio::test]
    async fn perplexity_webagent_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!(
                "skipping perplexity webagent integration (set FRIDAY_BROWSER_TEST=1 to enable)"
            );
            return;
        }
        let _guard = crate::adapters::browser::BROWSER_LIVE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let html = r#"<!doctype html><html><head><title>Echo</title></head>
            <body><textarea id="in"></textarea>
            <article id="out">initial</article>
            <script>
              document.getElementById('in').addEventListener('keydown', function (e) {
                if (e.key === 'Enter') {
                  document.getElementById('out').textContent = this.value;
                }
              });
            </script></body></html>"#;
        let (base, server) = serve_html(html.to_string()).await;

        let agent = PerplexityAdapter::new(BrowserAdapter::new());
        agent.open(&base).await.expect("open local page");
        agent
            .send_message("Hello Perplexity echo")
            .await
            .expect("send message");
        let resp = agent.read_response().await.expect("read response");
        assert_eq!(resp.title, "Echo");
        assert!(
            resp.text.contains("Hello Perplexity echo"),
            "got: {}",
            resp.text
        );
        let _ = server.abort();
    }

    /// Minimal static HTTP server for the browser test (same pattern as the
    /// browser adapter's own test).
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
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        let mut buf = [0u8; 4096];
                        let _ = sock.read(&mut buf).await;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    });
                }
            }
        });
        (format!("http://{addr}/index.html"), handle)
    }
}
