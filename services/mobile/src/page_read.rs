//! Reading a web page for an ask that names one ("summarize
//! https://example.com/post", "what does example.com say about pricing").
//!
//! FRIDAY's own browser (headless Chrome, a fresh profile, so none of the
//! person's logins) loads the page, scripts and all, and its visible text
//! goes to the answer step as the source. With no Chrome, a plain download
//! with the tags stripped does instead. Each read is logged in `egress.jsonl`
//! like a web search; private mode reads nothing.

use std::path::PathBuf;
use std::time::Duration;

use friday_actions::{ActionAdapter, ActionRequest, BrowserAdapter};
use serde_json::json;

/// Most characters of a page's text passed on to the model.
pub const MAX_CHARS: usize = 12_000;

/// A page as read.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub text: String,
}

#[async_trait::async_trait]
pub trait PageReader: Send + Sync {
    async fn read(&self, url: &str) -> Result<Page, String>;
}

/// Chrome first, then a plain download.
pub struct DefaultPageReader {
    browser: BrowserAdapter,
    client: reqwest::Client,
    egress_log_path: PathBuf,
}

impl Default for DefaultPageReader {
    fn default() -> Self {
        Self {
            browser: BrowserAdapter::new(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent(
                    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
                )
                .build()
                .unwrap_or_default(),
            egress_log_path: if cfg!(test) {
                std::env::temp_dir()
                    .join(format!("friday-test-egress-{}.jsonl", std::process::id()))
            } else {
                friday_intelligence::default_egress_path()
            },
        }
    }
}

impl DefaultPageReader {
    async fn with_chrome(&self, url: &str) -> Result<Page, String> {
        let opened = self
            .browser
            .execute(ActionRequest::new("web", "open", json!({ "url": url })))
            .await;
        if !opened.success {
            return Err(opened.error.unwrap_or_default());
        }
        let body = self
            .browser
            .execute(ActionRequest::new(
                "web",
                "extract",
                json!({ "selector": "body" }),
            ))
            .await;
        let text = body.output["text"].as_str().unwrap_or_default().trim();
        if !body.success || text.is_empty() {
            return Err(body.error.unwrap_or_else(|| "the page has no text".into()));
        }
        Ok(Page {
            url: opened.output["url"].as_str().unwrap_or(url).to_string(),
            title: opened.output["title"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            text: cap(&collapse(text)),
        })
    }

    async fn with_download(&self, url: &str) -> Result<Page, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("couldn't reach it ({e})"))?;
        if !resp.status().is_success() {
            return Err(format!("the site answered {}", resp.status()));
        }
        let final_url = resp.url().to_string();
        let html = resp.text().await.map_err(|e| e.to_string())?;
        let (title, text) = html_to_text(&html);
        if text.is_empty() {
            return Err("the page has no text without a browser".into());
        }
        Ok(Page {
            url: final_url,
            title,
            text: cap(&text),
        })
    }
}

#[async_trait::async_trait]
impl PageReader for DefaultPageReader {
    async fn read(&self, url: &str) -> Result<Page, String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only web addresses (http, https) can be read".into());
        }
        let chrome = tokio::time::timeout(Duration::from_secs(25), self.with_chrome(url))
            .await
            .unwrap_or_else(|_| Err("the browser took too long".into()));
        let result = match chrome {
            Ok(page) => Ok(page),
            Err(chrome) => {
                tracing::info!("page read: browser failed ({chrome}); downloading instead");
                self.with_download(url).await
            }
        };
        // A web page that leads to this computer or the home network (a
        // redirect to a router, a local service) isn't read: what's there
        // would go to the model. An address the person typed themselves
        // still is.
        let result = match result {
            Ok(page) if !reaches_local(url).await && reaches_local(&page.url).await => Err(
                "the page sent me to an address on your own network, so I didn't read it".into(),
            ),
            other => other,
        };
        let ctx = friday_intelligence::current_egress_context();
        friday_intelligence::append_entry(
            &self.egress_log_path,
            &friday_intelligence::EgressEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                goal_id: ctx.goal_id,
                provider: host(url).to_string(),
                model: "-".to_string(),
                kinds: vec!["page".to_string()],
                text_chars: url.chars().count(),
                image_bytes: 0,
                masked: ctx.masked,
                outcome: if result.is_ok() { "ok" } else { "error" }.to_string(),
                local: None,
            },
        );
        result
    }
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// Whether a host (with or without a port) is this computer or the local
/// network: localhost, `.local`-style names, loopback, private, link-local
/// and shared (CGNAT) addresses.
fn is_local(host: &str) -> bool {
    let name = split_host(host).0.to_ascii_lowercase();
    let name = name.trim_end_matches('.');
    if name == "localhost"
        || [".localhost", ".local", ".lan", ".home.arpa", ".internal"]
            .iter()
            .any(|s| name.ends_with(s))
    {
        return true;
    }
    name.parse().is_ok_and(local_ip)
}

/// A host without its user part, as (name, port).
fn split_host(host: &str) -> (&str, Option<u16>) {
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']').unwrap_or((rest, ""));
        return (name, after.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match host.rsplit_once(':') {
        Some((h, port)) if !h.contains(':') => (h, port.parse().ok()),
        _ => (host, None),
    }
}

fn local_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => local_v4(ip),
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some_and(local_v4)
        }
    }
}

/// [`is_local`], or a name that resolves to a local address
/// (`127.0.0.1.nip.io`, a home DNS entry). A name that doesn't resolve
/// counts as not local.
async fn reaches_local(url: &str) -> bool {
    let host = host(url);
    if is_local(host) {
        return true;
    }
    let (name, port) = split_host(host);
    let lookup = tokio::net::lookup_host((name, port.unwrap_or(80)));
    match tokio::time::timeout(Duration::from_secs(3), lookup).await {
        Ok(Ok(mut addrs)) => addrs.any(|a| local_ip(a.ip())),
        _ => false,
    }
}

fn local_v4(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || (o[0] == 100 && (o[1] & 0xc0) == 64)
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        out.push_str(&words.join(" "));
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn cap(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_CHARS).collect();
    out.push_str("\n[the rest of the page was cut]");
    out
}

/// The title and the visible text of an HTML page, roughly: scripts,
/// styles and comments go, block tags become line breaks, the common
/// entities are decoded.
pub fn html_to_text(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .and_then(|s| lower[s..].find("</title>").map(|e| (s, s + e)))
        .map(|(s, e)| collapse(&decode(&html[s..e])))
        .unwrap_or_default();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with("<!--") {
            i = lower[i..]
                .find("-->")
                .map(|e| i + e + 3)
                .unwrap_or(html.len());
            continue;
        }
        if html.as_bytes()[i] == b'<' {
            let end = lower[i..]
                .find('>')
                .map(|e| i + e + 1)
                .unwrap_or(html.len());
            let tag = lower[i + 1..end.saturating_sub(1).max(i + 1)]
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .next()
                .unwrap_or_default()
                .to_string();
            if ["script", "style", "noscript", "svg", "head", "title"].contains(&tag.as_str())
                && !lower[i + 1..].starts_with('/')
            {
                let close = format!("</{tag}");
                i = lower[end..]
                    .find(&close)
                    .and_then(|c| lower[end + c..].find('>').map(|g| end + c + g + 1))
                    .unwrap_or(html.len());
                continue;
            }
            if [
                "p",
                "div",
                "br",
                "li",
                "tr",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "section",
                "article",
                "header",
                "footer",
                "pre",
                "blockquote",
                "table",
                "ul",
                "ol",
            ]
            .contains(&tag.as_str())
            {
                out.push('\n');
            } else {
                out.push(' ');
            }
            i = end;
            continue;
        }
        let next = lower[i..].find('<').map(|n| i + n).unwrap_or(html.len());
        out.push_str(&html[i..next]);
        i = next;
    }
    (title, collapse(&decode(&out)))
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_its_visible_text() {
        let html = r#"<html><head><title>Pricing &amp; plans</title>
            <style>body{color:red}</style><script>var x = "<p>no</p>";</script></head>
            <body><!-- a comment --><h1>Plans</h1><p>Free:   0&nbsp;€</p>
            <ul><li>Pro: 10 €</li><li>Team</li></ul><noscript>turn on js</noscript></body></html>"#;
        let (title, text) = html_to_text(html);
        assert_eq!(title, "Pricing & plans");
        assert_eq!(text, "Plans\nFree: 0 €\nPro: 10 €\nTeam");
    }

    #[test]
    fn long_pages_are_cut_and_hosts_found() {
        let long = "word ".repeat(MAX_CHARS);
        assert!(cap(&long).ends_with("[the rest of the page was cut]"));
        assert_eq!(host("https://example.com/a?b"), "example.com");
        assert_eq!(host("http://127.0.0.1:8080"), "127.0.0.1:8080");
    }

    #[test]
    fn local_addresses_are_told_apart_from_the_web() {
        for h in [
            "localhost:8080",
            "127.0.0.1",
            "192.168.1.1",
            "10.0.0.5:80",
            "172.16.3.4",
            "169.254.169.254",
            "100.100.1.2",
            "[::1]:8080",
            "[fd00::1]",
            "[fe80::1]",
            "router.lan",
            "printer.local",
            "user@192.168.0.1",
            "user@[::1]:8080",
        ] {
            assert!(is_local(h), "{h}");
        }
        for h in [
            "example.com",
            "93.184.216.34",
            "[2001:db8::1]",
            "8.8.8.8:443",
        ] {
            assert!(!is_local(h), "{h}");
        }
    }

    #[tokio::test]
    async fn a_name_for_a_local_address_is_local() {
        assert!(reaches_local("http://localhost:9/x").await);
        assert!(!reaches_local("https://nosuch.invalid/").await);
    }

    #[tokio::test]
    async fn only_web_addresses_are_read() {
        let r = DefaultPageReader::default();
        assert!(r.read("file:///etc/passwd").await.is_err());
    }

    /// With no Chrome the page still comes through a plain download.
    #[tokio::test]
    async fn a_page_is_read_without_chrome() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body =
                    "<html><title>Docs</title><body><p>Install with pacman.</p></body></html>";
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let reader = DefaultPageReader {
            browser: BrowserAdapter::new().with_chrome_executable("/nonexistent/chrome"),
            egress_log_path: std::env::temp_dir()
                .join(format!("friday-page-egress-{}.jsonl", uuid::Uuid::new_v4())),
            ..DefaultPageReader::default()
        };
        let page = reader.read(&format!("http://{addr}/docs")).await.unwrap();
        assert_eq!(page.title, "Docs");
        assert_eq!(page.text, "Install with pacman.");
        let log = std::fs::read_to_string(&reader.egress_log_path).unwrap();
        assert!(
            log.contains("\"page\"") && log.contains("127.0.0.1"),
            "{log}"
        );
    }
}
