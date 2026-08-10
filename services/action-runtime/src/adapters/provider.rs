//! Provider identity (`specs/browser-organ.md` §4) — canonical expert IDs and
//! the registry that maps them to addresses + capability profiles.
//!
//! The design rule that drives this layer: **URLs change; identity doesn't.**
//! The cognitive core, the session pool, and the expert broker all speak
//! [`ProviderId`]; [`ProviderRegistry`] is the one place a provider's URL,
//! transport, and capability advertisement are defined. A URL is an
//! implementation detail of an identity — never a routing key.
//!
//! This layer's [`ProviderCapabilities`] are the *provider-selection*
//! vocabulary (image generation, citations, browser login, …). The
//! intelligence fabric's `ModelCapability` (`Json`/`Tools`/`Vision`/`Local`/
//! `LongContext`) is a *transport-level* sibling for model routing — the two
//! overlap (vision, tool_calling ≈ Tools, offline/api/local_memory ≈ Local)
//! but serve different decisions. Unifying them into one vocabulary is a
//! documented future decision; today each crate owns its own so neither
//! drifts against the other's consumers.
//!
//! ```text
//! ProviderId::ChatGPT  →  https://chatgpt.com   (browser, deep reasoning)
//! ProviderId::Grok     →  https://grok.com      (browser, realtime)
//! ProviderId::Gemini   →  https://gemini.google.com (browser, vision)
//! ProviderId::Claude   →  https://claude.ai     (browser, coding+deep)
//! ProviderId::Perplexity → https://www.perplexity.ai (browser, realtime+deep)
//! ProviderId::Local    →  http://localhost:8080 (local, privacy)
//! ```

use std::collections::HashMap;

/// How a provider reaches a model. The core stays transport-blind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Structured API (OpenAI, OmniRoute, … — owned by the intelligence fabric).
    Api,
    /// A logged-in website through the Browser Organ's dedicated profile.
    Browser,
    /// On-machine inference (llama.cpp, Ollama — owned by the intelligence fabric).
    Local,
}

impl Transport {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Browser => "browser",
            Self::Local => "local",
        }
    }
}

/// Scored expert profile: how strong each axis is, 0.0–1.0. This is the
/// transport-agnostic advertisement the router optimizes against — never a
/// provider name (routing by capability, not by vendor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpertProfile {
    /// Writing/reading code.
    pub coding: f32,
    /// Fresh, realtime, up-to-the-minute information.
    pub realtime: f32,
    /// Deep, careful, multi-step reasoning.
    pub deep_reasoning: f32,
    /// Numeric/mathematical work.
    pub math: f32,
    /// Image/visual understanding.
    pub vision: f32,
}

impl ExpertProfile {
    pub fn new(coding: f32, realtime: f32, deep: f32, math: f32, vision: f32) -> Self {
        Self {
            coding,
            realtime,
            deep_reasoning: deep,
            math,
            vision,
        }
    }
}

/// Canonical provider identity — the *why* of routing. The core never thinks
/// "open chatgpt.com"; it thinks "consult ChatGPT" (or just "external
/// reasoning" and lets the broker pick). [`Self::from_site`] owns the host
/// rules that used to live in `agent_for_site`; [`Self::from_name`] parses
/// env/param configuration. Serializes as its stable name (`"chatgpt"`), so
/// measured telemetry persists keyed by identity.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// OpenAI's ChatGPT (deep reasoning).
    ChatGPT,
    /// xAI's Grok (realtime).
    Grok,
    /// Google's Gemini (vision).
    Gemini,
    /// Anthropic's Claude (coding + deep reasoning).
    Claude,
    /// Perplexity (realtime + deep).
    Perplexity,
    /// On-machine inference (llama.cpp / Ollama — the intelligence fabric).
    Local,
    /// Everything else: an arbitrary site through the generic adapter.
    #[default]
    Generic,
}

impl ProviderId {
    /// Stable name for config, env vars, and the audit trail.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ChatGPT => "chatgpt",
            Self::Grok => "grok",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
            Self::Perplexity => "perplexity",
            Self::Local => "local",
            Self::Generic => "generic",
        }
    }

    /// Parse a provider name ("chatgpt", "grok", "local", …) — the identity
    /// side of the `provider` param and `FRIDAY_WEBAGENT_PROVIDER` env var.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "chatgpt" | "chat-gpt" | "openai" => Self::ChatGPT,
            "grok" | "xai" => Self::Grok,
            "gemini" | "google" => Self::Gemini,
            "claude" | "anthropic" => Self::Claude,
            "perplexity" => Self::Perplexity,
            "local" | "llamacpp" | "ollama" => Self::Local,
            "generic" | "web" => Self::Generic,
            _ => return None,
        })
    }

    /// Route a site URL to its canonical identity. Lookalike hosts are
    /// deliberately NOT mapped: `gemini.com` is the crypto exchange, not
    /// Google's AI product; `x.com`'s tweet composer stays generic so Grok's
    /// selectors can never type into a tweet box.
    pub fn from_site(site: &str) -> Self {
        let host = site
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if host == "chatgpt.com" || host.ends_with(".chatgpt.com") || host == "chat.openai.com" {
            Self::ChatGPT
        } else if host == "grok.com" || host.ends_with(".grok.com") {
            Self::Grok
        } else if host == "gemini.google.com" || host.ends_with(".gemini.google.com") {
            Self::Gemini
        } else if host == "claude.ai" || host.ends_with(".claude.ai") {
            Self::Claude
        } else if host == "perplexity.ai" || host.ends_with(".perplexity.ai") {
            Self::Perplexity
        } else {
            Self::Generic
        }
    }

    /// True for identities with a dedicated adapter/registry entry — the
    /// session pool keys these by identity. [`Self::Generic`] (and anything
    /// else) keys by canonical host so two arbitrary sites stay distinct.
    pub fn is_known(&self) -> bool {
        matches!(
            self,
            Self::ChatGPT | Self::Grok | Self::Gemini | Self::Claude | Self::Perplexity
        )
    }

    /// The canonical URL for this identity — one source of truth (the only
    /// place a provider's address lives besides the registry).
    pub fn default_url(&self) -> &'static str {
        match self {
            Self::ChatGPT => "https://chatgpt.com",
            Self::Grok => "https://grok.com",
            Self::Gemini => "https://gemini.google.com",
            Self::Claude => "https://claude.ai",
            Self::Perplexity => "https://www.perplexity.ai",
            Self::Local => "http://localhost:8080",
            Self::Generic => "",
        }
    }

    /// The default transport for this identity.
    pub fn default_transport(&self) -> Transport {
        if matches!(self, Self::Local) {
            Transport::Local
        } else {
            Transport::Browser
        }
    }

    /// The discrete capabilities this identity advertises — the hard gate
    /// before score-based ranking. A living list of **beliefs**, exactly like
    /// [`Self::default_profile`]: the same provider-qualification pipeline
    /// that tunes scores from measured evidence should eventually be able to
    /// flip these flags too.
    pub fn default_capabilities(&self) -> ProviderCapabilities {
        let mut c = ProviderCapabilities::default();
        match self {
            Self::ChatGPT => {
                c.vision = true;
                c.deep_reasoning = true;
                c.tool_calling = true;
                c.streaming = true;
                c.image_generation = true;
                c.multimodal = true;
                c.browser = true;
                c.browser_login = true;
            }
            Self::Grok => {
                c.vision = true;
                c.deep_reasoning = true;
                c.streaming = true;
                c.image_generation = true;
                c.browser = true;
                c.browser_login = true;
            }
            Self::Gemini => {
                c.vision = true;
                c.deep_reasoning = true;
                c.tool_calling = true;
                c.streaming = true;
                c.image_generation = true;
                c.multimodal = true;
                c.browser = true;
                c.browser_login = true;
            }
            Self::Claude => {
                c.vision = true;
                c.deep_reasoning = true;
                c.tool_calling = true;
                c.streaming = true;
                c.code_execution = true;
                c.citations = true;
                c.browser = true;
                c.browser_login = true;
            }
            Self::Perplexity => {
                c.citations = true;
                c.streaming = true;
                c.tool_calling = true;
                c.deep_reasoning = true;
                c.browser = true;
                c.browser_login = true;
            }
            Self::Local => {
                c.offline = true;
                c.local_memory = true;
                c.api = true;
                c.streaming = true;
            }
            Self::Generic => {}
        }
        c
    }

    /// The default capability profile this identity advertises; the broker
    /// optimizes against it. A living list — replace with measured evidence
    /// from provider qualification, never a hardcoded vendor belief.
    pub fn default_profile(&self) -> ExpertProfile {
        match self {
            Self::ChatGPT => ExpertProfile::new(0.7, 0.2, 0.95, 0.6, 0.1),
            Self::Grok => ExpertProfile::new(0.5, 0.9, 0.4, 0.3, 0.1),
            Self::Gemini => ExpertProfile::new(0.6, 0.5, 0.8, 0.7, 0.9),
            Self::Claude => ExpertProfile::new(0.9, 0.2, 0.9, 0.4, 0.2),
            Self::Perplexity => ExpertProfile::new(0.4, 0.9, 0.6, 0.3, 0.2),
            Self::Local => ExpertProfile::new(0.8, 0.0, 0.6, 0.7, 0.1),
            Self::Generic => ExpertProfile::new(0.5, 0.5, 0.5, 0.5, 0.5),
        }
    }
}

/// A discrete thing a provider can do — *whether* it can, as opposed to
/// [`ExpertProfile`]'s scored axes (*how well*). Routing gates on these first
/// (a provider without `vision` is never offered a vision question), then
/// ranks survivors by score. A living list — replace with measured evidence
/// from provider qualification, never a hardcoded vendor belief.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilities {
    pub vision: bool,
    pub deep_reasoning: bool,
    pub tool_calling: bool,
    pub streaming: bool,
    pub citations: bool,
    pub image_generation: bool,
    pub code_execution: bool,
    pub local_memory: bool,
    pub offline: bool,
    pub browser: bool,
    pub voice: bool,
    pub multimodal: bool,
    pub api: bool,
    pub browser_login: bool,
}

/// One capability flag, addressable by name (params, config, audit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderCapability {
    Vision,
    DeepReasoning,
    ToolCalling,
    Streaming,
    Citations,
    ImageGeneration,
    CodeExecution,
    LocalMemory,
    Offline,
    Browser,
    Voice,
    Multimodal,
    Api,
    BrowserLogin,
}

impl ProviderCapability {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Vision => "vision",
            Self::DeepReasoning => "deep_reasoning",
            Self::ToolCalling => "tool_calling",
            Self::Streaming => "streaming",
            Self::Citations => "citations",
            Self::ImageGeneration => "image_generation",
            Self::CodeExecution => "code_execution",
            Self::LocalMemory => "local_memory",
            Self::Offline => "offline",
            Self::Browser => "browser",
            Self::Voice => "voice",
            Self::Multimodal => "multimodal",
            Self::Api => "api",
            Self::BrowserLogin => "browser_login",
        }
    }

    /// Parse a capability name (the `requires` param). `"local"` is
    /// deliberately NOT an alias for `offline`: `provider: "local"` already
    /// means the local *identity*, so `requires: "local"` meaning an offline
    /// gate would be a silent footgun — be explicit (`"offline"`).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "vision" | "image_input" => Self::Vision,
            "deep_reasoning" | "reasoning" => Self::DeepReasoning,
            "tool_calling" | "tools" => Self::ToolCalling,
            "streaming" => Self::Streaming,
            "citations" | "cites" => Self::Citations,
            "image_generation" | "images" => Self::ImageGeneration,
            "code_execution" | "code" => Self::CodeExecution,
            "local_memory" | "memory" => Self::LocalMemory,
            "offline" => Self::Offline,
            "browser" => Self::Browser,
            "voice" => Self::Voice,
            "multimodal" => Self::Multimodal,
            "api" => Self::Api,
            "browser_login" | "login" => Self::BrowserLogin,
            _ => return None,
        })
    }
}

impl ProviderCapabilities {
    /// Every capability in stable audit order — the single list both
    /// [`Self::enabled`] and the round-trip test derive from, so adding a flag
    /// touches this array once plus the struct field and match arms.
    pub const ALL: [ProviderCapability; 14] = [
        ProviderCapability::Vision,
        ProviderCapability::DeepReasoning,
        ProviderCapability::ToolCalling,
        ProviderCapability::Streaming,
        ProviderCapability::Citations,
        ProviderCapability::ImageGeneration,
        ProviderCapability::CodeExecution,
        ProviderCapability::LocalMemory,
        ProviderCapability::Offline,
        ProviderCapability::Browser,
        ProviderCapability::Voice,
        ProviderCapability::Multimodal,
        ProviderCapability::Api,
        ProviderCapability::BrowserLogin,
    ];

    pub fn supports(&self, cap: ProviderCapability) -> bool {
        match cap {
            ProviderCapability::Vision => self.vision,
            ProviderCapability::DeepReasoning => self.deep_reasoning,
            ProviderCapability::ToolCalling => self.tool_calling,
            ProviderCapability::Streaming => self.streaming,
            ProviderCapability::Citations => self.citations,
            ProviderCapability::ImageGeneration => self.image_generation,
            ProviderCapability::CodeExecution => self.code_execution,
            ProviderCapability::LocalMemory => self.local_memory,
            ProviderCapability::Offline => self.offline,
            ProviderCapability::Browser => self.browser,
            ProviderCapability::Voice => self.voice,
            ProviderCapability::Multimodal => self.multimodal,
            ProviderCapability::Api => self.api,
            ProviderCapability::BrowserLogin => self.browser_login,
        }
    }

    /// True when every required capability is present (empty list → true).
    pub fn has_all(&self, required: &[ProviderCapability]) -> bool {
        required.iter().all(|c| self.supports(*c))
    }

    /// The names of every enabled capability (for the audit trail).
    pub fn enabled(&self) -> Vec<&'static str> {
        Self::ALL
            .into_iter()
            .filter(|c| self.supports(*c))
            .map(|c| c.name())
            .collect()
    }
}

/// One registry row: an identity's address, transport, capability profile, and
/// discrete capability set.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEntry {
    pub id: ProviderId,
    pub url: String,
    pub transport: Transport,
    pub profile: ExpertProfile,
    pub capabilities: ProviderCapabilities,
}

/// Maps provider identity → {url, transport, profile}. The one place a
/// provider's address and capability advertisement live; everyone else speaks
/// [`ProviderId`]. Deployments extend with [`Self::insert`]; the default
/// covers the shipped identities.
#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    entries: HashMap<ProviderId, ProviderEntry>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// The shipped registry: the five browser identities + Local, each with
    /// its canonical URL, transport, capability profile, and discrete
    /// capability set.
    pub fn default_browser() -> Self {
        let mut reg = Self::new();
        for id in [
            ProviderId::ChatGPT,
            ProviderId::Grok,
            ProviderId::Gemini,
            ProviderId::Claude,
            ProviderId::Perplexity,
            ProviderId::Local,
        ] {
            reg.insert(ProviderEntry {
                id,
                url: id.default_url().to_string(),
                transport: id.default_transport(),
                profile: id.default_profile(),
                capabilities: id.default_capabilities(),
            });
        }
        reg
    }

    pub fn insert(&mut self, entry: ProviderEntry) {
        self.entries.insert(entry.id, entry);
    }

    pub fn get(&self, id: ProviderId) -> Option<&ProviderEntry> {
        self.entries.get(&id)
    }

    pub fn url(&self, id: ProviderId) -> Option<&str> {
        self.entries.get(&id).map(|e| e.url.as_str())
    }

    pub fn profile(&self, id: ProviderId) -> Option<ExpertProfile> {
        self.entries.get(&id).map(|e| e.profile)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_site_maps_known_hosts_to_identity() {
        assert_eq!(
            ProviderId::from_site("https://chatgpt.com"),
            ProviderId::ChatGPT
        );
        assert_eq!(
            ProviderId::from_site("https://chat.openai.com/"),
            ProviderId::ChatGPT
        );
        assert_eq!(
            ProviderId::from_site("https://grok.com/chat"),
            ProviderId::Grok
        );
        assert_eq!(
            ProviderId::from_site("https://gemini.google.com/app"),
            ProviderId::Gemini
        );
        assert_eq!(
            ProviderId::from_site("https://claude.ai"),
            ProviderId::Claude
        );
        assert_eq!(
            ProviderId::from_site("https://www.perplexity.ai"),
            ProviderId::Perplexity
        );
    }

    #[test]
    fn from_site_never_hijacks_lookalike_hosts() {
        // x.com's tweet composer matches Grok's selectors — identity must stay
        // generic so nothing routes to the Grok adapter.
        assert_eq!(ProviderId::from_site("https://x.com/"), ProviderId::Generic);
        // gemini.com is the crypto exchange, not Google's AI product.
        assert_eq!(
            ProviderId::from_site("https://gemini.com"),
            ProviderId::Generic
        );
        assert_eq!(
            ProviderId::from_site("https://example.com"),
            ProviderId::Generic
        );
    }

    #[test]
    fn from_name_roundtrips() {
        assert_eq!(ProviderId::from_name("chatgpt"), Some(ProviderId::ChatGPT));
        assert_eq!(ProviderId::from_name("ChatGPT "), Some(ProviderId::ChatGPT));
        assert_eq!(ProviderId::from_name("grok"), Some(ProviderId::Grok));
        assert_eq!(ProviderId::from_name("claude"), Some(ProviderId::Claude));
        assert_eq!(
            ProviderId::from_name("perplexity"),
            Some(ProviderId::Perplexity)
        );
        assert_eq!(ProviderId::from_name("local"), Some(ProviderId::Local));
        assert_eq!(ProviderId::from_name("not-a-provider"), None);
    }

    #[test]
    fn default_registry_exposes_url_transport_and_profile() {
        let reg = ProviderRegistry::default_browser();
        assert_eq!(reg.len(), 6);
        let chatgpt = reg.get(ProviderId::ChatGPT).expect("chatgpt registered");
        assert_eq!(chatgpt.url, "https://chatgpt.com");
        assert_eq!(chatgpt.transport, Transport::Browser);
        assert!(
            chatgpt.profile.deep_reasoning > 0.9,
            "deep reasoning is ChatGPT's known strength"
        );
        assert_eq!(
            reg.url(ProviderId::Grok),
            Some("https://grok.com"),
            "the registry is the one place addresses live"
        );
        let grok = reg.profile(ProviderId::Grok).expect("grok profile");
        assert!(grok.realtime > 0.8, "realtime is Grok's known strength");
        assert_eq!(
            reg.get(ProviderId::Local)
                .expect("local registered")
                .transport,
            Transport::Local
        );
        assert_eq!(
            reg.get(ProviderId::Generic),
            None,
            "generic has no registry row"
        );
    }

    #[test]
    fn capability_defaults_gate_by_identity() {
        let chatgpt = ProviderId::ChatGPT.default_capabilities();
        assert!(chatgpt.supports(ProviderCapability::ImageGeneration));
        assert!(chatgpt.supports(ProviderCapability::Streaming));
        assert!(chatgpt.supports(ProviderCapability::Browser));
        assert!(!chatgpt.supports(ProviderCapability::Offline));

        // Perplexity's hallmark is citations.
        let perplexity = ProviderId::Perplexity.default_capabilities();
        assert!(perplexity.supports(ProviderCapability::Citations));

        // Local is offline and API-driven; it is NOT a browser identity.
        let local = ProviderId::Local.default_capabilities();
        assert!(local.supports(ProviderCapability::Offline));
        assert!(local.supports(ProviderCapability::Api));
        assert!(!local.supports(ProviderCapability::Browser));

        // Generic advertises nothing — no claims, no gate hits.
        let generic = ProviderId::Generic.default_capabilities();
        assert!(!generic.supports(ProviderCapability::Vision));
        assert!(generic.enabled().is_empty());
    }

    #[test]
    fn has_all_requires_every_capability() {
        let c = ProviderId::ChatGPT.default_capabilities();
        assert!(c.has_all(&[]));
        assert!(c.has_all(&[ProviderCapability::Vision, ProviderCapability::Streaming]));
        assert!(!c.has_all(&[ProviderCapability::Vision, ProviderCapability::Offline]));
        assert_eq!(
            ProviderCapability::from_name("vision"),
            Some(ProviderCapability::Vision)
        );
        assert_eq!(ProviderCapability::from_name("nope"), None);
        // "local" is NOT an offline alias — that would collide with the local
        // provider identity; be explicit.
        assert_eq!(ProviderCapability::from_name("local"), None);
    }

    #[test]
    fn capability_names_roundtrip_and_are_unique() {
        // Every variant parses back from its own name (no name()/from_name()
        // drift), and enabled() output is unique and parseable.
        for cap in ProviderCapabilities::ALL {
            assert_eq!(ProviderCapability::from_name(cap.name()), Some(cap));
        }
        let mut seen = std::collections::HashSet::new();
        for name in ProviderId::ChatGPT.default_capabilities().enabled() {
            assert!(seen.insert(name), "duplicate capability name: {name}");
            assert!(ProviderCapability::from_name(name).is_some());
        }
    }
}
