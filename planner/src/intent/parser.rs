use super::classifier::IntentType;
use super::goal::Intent;

/// Turn a raw user utterance into a structured [`Intent`].
///
/// The parser is deterministic and rule-based: it classifies the intent, then
/// extracts the entities relevant to that intent. Replacing this with an LLM
/// later means swapping this one function — the `Intent` shape stays constant.
pub fn parse(raw: &str) -> Intent {
    let kind = IntentType::classify(raw);
    let mut intent = Intent::new(kind.as_intent(), raw);

    match kind {
        IntentType::BrowserSearch => {
            let browser = detect_browser(raw).unwrap_or_else(|| "default".to_string());
            let query = extract_after(raw, "search").unwrap_or_default();
            intent = intent
                .with_entity("browser", serde_json::json!(browser))
                .with_entity("query", serde_json::json!(query));
        }
        IntentType::OpenApp => {
            if let Some(app) = extract_after(raw, "open") {
                intent = intent.with_entity("app", serde_json::json!(app));
            }
        }
        _ => {}
    }
    intent
}

fn detect_browser(raw: &str) -> Option<String> {
    let r = raw.to_ascii_lowercase();
    for name in ["chrome", "chromium", "edge", "firefox", "brave", "opera"] {
        if r.contains(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Return the trimmed text that follows the first occurrence of `keyword`.
fn extract_after(raw: &str, keyword: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if let Some(idx) = lower.find(keyword) {
        let rest = &raw[idx + keyword.len()..];
        let rest = rest.trim_start_matches([' ', '\t']);
        let rest = rest.split(['.', '!', '?']).next().unwrap_or(rest).trim();
        if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    } else {
        None
    }
}
