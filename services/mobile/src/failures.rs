//! Plain-language failures for the person who asked.
//!
//! Provider and capture errors are written for logs ("HTTP request failed:
//! error sending request for url (...): tcp connect error: Connection
//! refused"). What reaches the notification or the answer window is one
//! sentence that says what went wrong and what to do; the raw error stays in
//! the daemon's log.

use crate::context_read::ContextReadError;

/// Say why the model gave no answer, from the fabric's raw error. OpenCode
/// Zen's free-limit error passes through unchanged: each desktop explains it
/// in its own words (where to add a key differs).
pub fn plain_model_error(raw: &str) -> String {
    if raw.contains("FreeUsageLimitError") {
        return raw.to_string();
    }
    let r = raw.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| r.contains(n));
    if has(&["none of your ai providers"]) {
        // Already written for people (the screen-route refusal).
        return raw.to_string();
    }
    if has(&["no eligible providers"]) {
        // Every provider failed recently (their circuits are open) or none
        // fits the request.
        return "None of your AI providers is reachable right now. Check your internet \
                connection (or your local model server), or choose another provider with \
                `friday setup-ai` (on Windows: AI provider in the tray menu)."
            .to_string();
    }
    if has(&["no model registered", "not registered"]) {
        return "No AI provider is available. Choose one with `friday setup-ai` \
                (on Windows: AI provider in the tray menu)."
            .to_string();
    }
    if has(&["timed out", "timeout"]) {
        return "Your AI provider didn't answer in time. Try again in a moment.".to_string();
    }
    if has(&[
        " 401",
        " 403",
        "unauthorized",
        "invalid api key",
        "invalid_api_key",
    ]) {
        return "Your AI provider turned the key down. Check it with `friday setup-ai` \
                (on Windows: AI provider in the tray menu)."
            .to_string();
    }
    if has(&[
        "error sending request",
        "connection refused",
        "actively refused",
        "dns error",
        "failed to lookup",
        "tcp connect",
        "network is unreachable",
        "connection reset",
    ]) {
        return "Couldn't reach your AI provider. Check your internet connection \
                (or that your local model server is running)."
            .to_string();
    }
    let short: String = raw.chars().take(160).collect();
    format!("Your AI provider couldn't answer ({short}).")
}

/// Say why a screen ask gave no answer.
pub fn plain_screen_error(e: &ContextReadError) -> String {
    match e {
        ContextReadError::Capture(m) => {
            let m = m.to_ascii_lowercase();
            if m.contains("no such file") || m.contains("not found") || m.contains("cannot find") {
                "Couldn't take a screenshot: the screenshot tool isn't installed \
                 (grim on Linux; on Windows, reinstall FRIDAY)."
                    .to_string()
            } else {
                "Couldn't take a screenshot. Nothing was sent.".to_string()
            }
        }
        ContextReadError::NotAuthorized { .. } => {
            "The screenshot needs your go-ahead each time; nothing was sent.".to_string()
        }
        ContextReadError::EgressBlocked { .. } => {
            "The screenshot looked like it held a secret, so it wasn't sent.".to_string()
        }
        ContextReadError::InvalidPayload(_) => "Couldn't read the screenshot.".to_string(),
        ContextReadError::Provider(raw) => plain_model_error(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_errors_become_one_plain_sentence() {
        let unreachable = "HTTP request failed: error sending request for url \
            (http://127.0.0.1:9/v1/chat/completions): tcp connect error: Connection refused";
        assert!(plain_model_error(unreachable).starts_with("Couldn't reach your AI provider"));
        assert!(plain_model_error("request timed out: no answer for 15s")
            .contains("didn't answer in time"));
        assert!(plain_model_error("No eligible providers for this request.")
            .contains("None of your AI providers is reachable"));
        assert!(plain_model_error("provider gemini not registered")
            .contains("No AI provider is available"));
        assert!(
            plain_model_error("Provider groq returned error status 401 Unauthorized: x")
                .contains("turned the key down")
        );
        let other = plain_model_error("something odd");
        assert_eq!(other, "Your AI provider couldn't answer (something odd).");
    }

    #[test]
    fn some_errors_pass_through() {
        let limit = "Provider opencode returned error status 429: FreeUsageLimitError";
        assert_eq!(plain_model_error(limit), limit);
        let refusal = "None of your AI providers (llamacpp) can see images, so ...";
        assert_eq!(plain_model_error(refusal), refusal);
    }

    #[test]
    fn screen_errors_say_what_happened() {
        let missing = ContextReadError::Capture(
            "capture command failed for channel 'screen': spawn 'grim': No such file or directory"
                .into(),
        );
        assert!(plain_screen_error(&missing).contains("screenshot tool isn't installed"));
        let failed = ContextReadError::Capture("grim exited 1: no outputs".into());
        assert_eq!(
            plain_screen_error(&failed),
            "Couldn't take a screenshot. Nothing was sent."
        );
        let provider = ContextReadError::Provider("request timed out: no answer for 15s".into());
        assert!(plain_screen_error(&provider).contains("didn't answer in time"));
    }
}
