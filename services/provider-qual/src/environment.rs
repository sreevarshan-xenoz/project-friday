//! Environment capture for Evidence Packs.
//!
//! `environment.json` records the provider configuration a qualification run
//! happened against, so a failure years later is debuggable without reproducing
//! the environment. No PowerShell is needed — provider config comes from the
//! process environment.

use serde::Serialize;

use crate::harness::Mode;

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub mode: String,
    pub timestamp: String,
    pub friday_version: String,
    pub provider_name: String,
    pub provider_base_url: String,
    pub model_id: String,
    pub os: String,
    /// Which provider API keys / base URLs were present in the environment.
    pub configured_providers: Vec<String>,
}

pub fn capture(mode: Mode, provider_name: &str, base_url: &str, model_id: &str) -> Environment {
    let mode_str = match mode {
        Mode::Mock => "mock",
        Mode::Real => "real",
    };

    let mut configured = Vec::new();
    for (key, name) in [
        ("OMNIROUTE_API_KEY", "omniroute"),
        ("OPENROUTER_API_KEY", "openrouter"),
        ("GROQ_API_KEY", "groq"),
        ("CEREBRAS_API_KEY", "cerebras"),
        ("GEMINI_API_KEY", "gemini"),
        ("OPENGATEWAY_API_KEY", "opengateway"),
    ] {
        if std::env::var(key).is_ok() {
            configured.push(name.to_string());
        }
    }
    if std::env::var("OMNIROUTE_BASE_URL").is_ok() {
        configured.push("omniroute(base_url)".to_string());
    }
    if std::env::var("OPENGATEWAY_BASE_URL").is_ok() {
        configured.push("opengateway(base_url)".to_string());
    }

    Environment {
        mode: mode_str.into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        friday_version: env!("CARGO_PKG_VERSION").into(),
        provider_name: provider_name.into(),
        provider_base_url: base_url.into(),
        model_id: model_id.into(),
        os: std::env::consts::OS.into(),
        configured_providers: configured,
    }
}
