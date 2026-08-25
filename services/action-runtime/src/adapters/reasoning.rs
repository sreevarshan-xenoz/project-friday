//! ReasoningProvider layer (`specs/browser-organ.md` §3c) — a
//! transport-agnostic "I need another intelligence" interface.
//!
//! The cognitive core never thinks "open chatgpt.com". It thinks "I need
//! external reasoning", and the [`ExpertRouter`] picks the best-suited expert.
//! To the core, an API key and a logged-in browser profile are identical:
//! every provider answers [`ReasoningProvider::reason`] with text.
//!
//! ```text
//! ReasoningProvider
//!   ├── Api     (OpenAI / OmniRoute — the intelligence fabric's transports)
//!   ├── Browser (logged-in sites via the Browser Organ — implemented here)
//!   └── Local   (llama.cpp / Ollama — the intelligence fabric's transports)
//! ```
//!
//! Only the **Browser** transport ships in this crate (it is the one the
//! dedicated-profile session model unlocks). API and Local providers belong to
//! `friday-intelligence`'s fabric — the trait is the seam that makes them
//! interchangeable, not a duplicate registry.
//!
//! ## One contract: [`ReasoningRequest`]
//!
//! The router's public surface is request-driven, never provider-driven:
//! callers build a [`ReasoningRequest`] describing *what they need* (question,
//! required capabilities, latency/confidence preferences) and a
//! [`RoutingPolicy`], then call `best(request)` / `consult(request)`. The
//! router — not the caller — maps that to a provider. Provider identities stay
//! **implementation details**; the planner only names one deliberately
//! (`Prefer`/`Force`) for a real reason (user asked, A/B, debugging,
//! benchmarking, a provider-specific capability). No target-provider overloads
//! exist on the public API — `routing` lives in the request, so future
//! strategies (`Consensus`, `Cheapest`, …) extend the request, never the
//! method list.
//!
//! Security invariants inherited from the organ: providers never expose
//! credentials (the browser profile IS the credential boundary), page content
//! is untrusted data (a reply is a *string*, never an instruction), and the
//! router only ever asks — it cannot be told to click, type, or navigate.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::adapters::provider::{ProviderCapabilities, ProviderCapability, ProviderId};
use crate::adapters::telemetry::{MeasuredOutcome, ProviderStats, ProviderTelemetry};
use crate::adapters::{ActionAdapter, WebAgent, WebAgentResponse};
use crate::request::{ActionRequest, ActionResult};

/// [`Transport`] and [`ExpertProfile`] now live in [`crate::adapters::provider`]
/// (with the identity registry) and are re-exported here so existing imports
/// (`friday_actions::{Transport, ExpertProfile}`) keep resolving.
pub use crate::adapters::provider::{ExpertProfile, Transport};

/// One swappable external intelligence. Implementors pick their own
/// [`Transport`]; the caller never knows (or cares) which.
#[async_trait]
pub trait ReasoningProvider: Send + Sync {
    /// Stable identity (for the audit trail, not for routing).
    fn id(&self) -> &str;
    /// How this provider reaches its model.
    fn transport(&self) -> Transport;
    /// The capability strengths this expert advertises.
    fn profile(&self) -> ExpertProfile;
    /// The canonical identity whose measured evidence this provider's outcomes
    /// count toward. Defaults to [`ProviderId::Generic`]; browser experts
    /// advertise their site's identity so telemetry keys correctly.
    fn provider_id(&self) -> ProviderId {
        ProviderId::Generic
    }
    /// The discrete capabilities this provider actually offers — the hard
    /// gate before score-based ranking. Defaults from the canonical identity's
    /// defaults; implementors with a [`ProviderId::Generic`] identity
    /// advertise nothing unless they override.
    fn capabilities(&self) -> ProviderCapabilities {
        self.provider_id().default_capabilities()
    }
    /// Ask the expert and return its reply as plain text.
    async fn reason(&self, question: &str) -> Result<String, String>;
}

/// The Browser transport: wraps a [`WebAgent`] (a logged-in site) behind the
/// uniform [`ReasoningProvider`] contract. Site selection happens at
/// construction (`agent_for_site`); the router only ever calls [`reason`].
///
/// [`reason`]: ReasoningProvider::reason
pub struct BrowserReasoningProvider {
    id: String,
    /// The canonical identity this expert advertises (telemetry keys on it).
    provider: ProviderId,
    /// The site this expert talks to (`agent_for_site` routes it).
    url: String,
    agent: Box<dyn WebAgent>,
    profile: ExpertProfile,
}

impl BrowserReasoningProvider {
    /// Wrap a site adapter with the profile its site is known for. Profiles are
    /// a living list — like selectors, they degrade loudly, never silently.
    /// The identity is derived from the site ([`ProviderId::from_site`]), so
    /// measured evidence lands under the right key.
    pub fn new(url: &str, agent: Box<dyn WebAgent>, profile: ExpertProfile) -> Self {
        Self {
            id: format!("browser:{}", agent.site()),
            provider: ProviderId::from_site(url),
            url: url.to_string(),
            agent,
            profile,
        }
    }

    /// The ChatGPT vertical slice: deep reasoning is its known strength.
    pub fn chatgpt(profile_dir: std::path::PathBuf) -> Self {
        Self::for_id(ProviderId::ChatGPT, profile_dir)
    }

    /// The realtime expert: grok.com via its dedicated adapter (a
    /// `GrokAdapter` now routes through `agent_for_site`).
    pub fn grok(profile_dir: std::path::PathBuf) -> Self {
        Self::for_id(ProviderId::Grok, profile_dir)
    }

    /// The vision-capable expert: gemini.google.com via its dedicated
    /// adapter (a `GeminiAdapter` routes through `agent_for_site`).
    pub fn gemini(profile_dir: std::path::PathBuf) -> Self {
        Self::for_id(ProviderId::Gemini, profile_dir)
    }

    /// Build a browser-transport expert for a canonical identity: the URL and
    /// capability profile come from the identity's own defaults — the same
    /// data [`ProviderRegistry::default_browser`] materializes (one source of
    /// truth — a provider's address never lives in two places).
    ///
    /// This constructor binds to identity defaults; deployments that
    /// re-point an identity's address do so on the planner-facing seam
    /// ([`WebAgentAdapter::with_registry`]) rather than per-provider here.
    pub fn for_id(id: ProviderId, profile_dir: std::path::PathBuf) -> Self {
        let url = id.default_url();
        let agent = crate::adapters::agent_for_site(
            url,
            crate::adapters::BrowserAdapter::new().with_user_data_dir(profile_dir),
        );
        let profile = id.default_profile();
        Self {
            id: format!("browser:{}", agent.site()),
            provider: id,
            url: url.to_string(),
            agent,
            profile,
        }
    }
}

#[async_trait]
impl ReasoningProvider for BrowserReasoningProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn transport(&self) -> Transport {
        Transport::Browser
    }

    fn profile(&self) -> ExpertProfile {
        self.profile
    }

    fn provider_id(&self) -> ProviderId {
        self.provider
    }

    async fn reason(&self, question: &str) -> Result<String, String> {
        // The WebAgent's `ask` default does not open the site — the provider
        // must navigate to its own site first (like WebAgentAdapter's
        // `ensure_agent` does).
        self.agent.open(&self.url).await?;
        let resp: WebAgentResponse = self.agent.ask(question).await?;
        Ok(resp.text)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// ExpertRouter — "FRIDAY chooses the expert"
// ───────────────────────────────────────────────────────────────────────────

/// A reply plus which expert produced it (the audit trail records the *why*).
#[derive(Debug, Clone)]
pub struct ConsultResult {
    /// The expert's answer.
    pub text: String,
    /// Which provider answered (`browser:chatgpt.com`, …).
    pub provider: String,
    /// How it reached the model (`browser`, `api`, `local`).
    pub transport: Transport,
    /// The provider's own profile for the winning axis (transparency).
    pub profile: ExpertProfile,
    /// The discrete capabilities the winner actually offers (the gate that let
    /// it through).
    pub capabilities: ProviderCapabilities,
    /// Measured evidence for the winning provider at the time of the consult
    /// (success rate / latency) — the measured *why* behind the choice.
    pub measured: Option<ProviderStats>,
}

/// How the router should pick an expert for a [`ReasoningRequest`]. The
/// planner expresses *intent*, never a vendor: [`Self::Automatic`] lets the
/// router decide by evidence-blended profile fit; [`Self::Prefer`] promotes a
/// target but still fails over in rank order; [`Self::Force`] is the
/// deliberate override (user asked, A/B test, debugging, benchmarking, a
/// provider-specific capability) and **never substitutes** — the forced expert
/// or an error. [`Self::Fastest`] makes measured latency the objective.
///
/// `Consensus`/`Cheapest` are **reserved**: they route to a loud, explicit
/// error until multi-expert combination and cost telemetry ship. A request
/// that cannot be satisfied must fail loudly, never silently become something
/// else — exactly like an unknown required capability.
///
/// This is expert *selection* policy, distinct from the intelligence fabric's
/// parked `RoutingPolicy` trait (a *model* routing optimization over
/// `RequestRequirements`); the two layers keep their own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingPolicy {
    /// Profile-fit selection (the default) — "FRIDAY chooses the expert".
    #[default]
    Automatic,
    /// Promote the target to the front of the rank; failover proceeds in rank
    /// order after it. An unknown/unregistered target is ignored (profile wins).
    Prefer(ProviderId),
    /// Only this expert is eligible — error if unregistered, gate-failing, or
    /// itself failing. No failover: a forced answer is the point.
    Force(ProviderId),
    /// Reserved — ask several experts and combine. Errors until implemented.
    Consensus,
    /// Reserved — cheapest provider first. Errors until cost telemetry exists.
    Cheapest,
    /// Measured-latency objective: fastest measured expert first (unmeasured
    /// providers rank after measured ones).
    Fastest,
}

impl RoutingPolicy {
    /// Stable audit name (`"automatic"`, `"prefer"`, `"force"`, …).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Prefer(_) => "prefer",
            Self::Force(_) => "force",
            Self::Consensus => "consensus",
            Self::Cheapest => "cheapest",
            Self::Fastest => "fastest",
        }
    }

    /// The identity this policy pins ([`Self::Prefer`]/[`Self::Force`]), if any.
    pub fn target(&self) -> Option<ProviderId> {
        match self {
            Self::Prefer(id) | Self::Force(id) => Some(*id),
            _ => None,
        }
    }
}

/// A preference (never a hard constraint) for how fast the winner should be.
/// Applied as a bounded ordering boost on top of the routing policy — a
/// provider with strong measured evidence of being fast is nudged up; the
/// policy's objective still rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LatencyTier {
    /// No latency preference.
    #[default]
    Any,
    /// Prefer the fastest measured provider (a preference, not a bound).
    Fast,
}

/// A preference (never a hard constraint) for how proven the winner should be.
/// Applied as a bounded ordering boost on top of the routing policy: providers
/// with lots of successful measured evidence are nudged up; unproven providers
/// are not excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfidenceTier {
    /// No confidence preference.
    #[default]
    Any,
    /// Prefer the provider with the strongest measured success record.
    High,
}

/// What a consult needs — the single public contract of the reasoning layer.
///
/// The caller describes *needs*, not vendors: `question` is the ask, `requires`
/// is the hard capability gate, `routing` is how to pick (default
/// [`RoutingPolicy::Automatic`]), and `latency`/`confidence` are preference
/// tiers that nudge the order. Providers never appear here except inside a
/// deliberate `Prefer`/`Force` policy. Adding future knobs (cost bounds, a
/// minimum-confidence floor, consensus fan-out) extends *this struct*, never
/// the router's method list.
#[derive(Debug, Clone, Default)]
pub struct ReasoningRequest {
    /// The question to ask.
    pub question: String,
    /// How to pick an expert.
    pub routing: RoutingPolicy,
    /// Hard capability gate — providers lacking any are excluded before
    /// ranking. Empty means no constraint.
    pub requires: Vec<ProviderCapability>,
    /// Latency preference tier (default [`LatencyTier::Any`]).
    pub latency: LatencyTier,
    /// Confidence preference tier (default [`ConfidenceTier::Any`]).
    pub confidence: ConfidenceTier,
}

impl ReasoningRequest {
    pub fn new(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            routing: RoutingPolicy::Automatic,
            requires: Vec::new(),
            latency: LatencyTier::Any,
            confidence: ConfidenceTier::Any,
        }
    }

    pub fn with_routing(mut self, routing: RoutingPolicy) -> Self {
        self.routing = routing;
        self
    }

    pub fn requires(mut self, caps: &[ProviderCapability]) -> Self {
        self.requires = caps.to_vec();
        self
    }

    pub fn with_latency(mut self, latency: LatencyTier) -> Self {
        self.latency = latency;
        self
    }

    pub fn with_confidence(mut self, confidence: ConfidenceTier) -> Self {
        self.confidence = confidence;
        self
    }
}

/// Keyword groups per expertise axis. Deterministic and cheap — a model is
/// welcome to enrich the classification later, but routing itself must be
/// reproducible (same question → same expert), exactly like the investigation
/// heuristic.
const CODE_KEYWORDS: &[&str] = &[
    "code",
    "rust",
    "function",
    "compile",
    "debug",
    "refactor",
    "api",
    "crash",
    "exception",
    "syntax",
    "variable",
    "loop",
    "test fail",
];
const REALTIME_KEYWORDS: &[&str] = &[
    "latest", "news", "today", "current", "now", "score", "price", "weather", "breaking", "update",
];
const DEEP_KEYWORDS: &[&str] = &[
    "explain",
    "why",
    "analyze",
    "compare",
    "derive",
    "reason",
    "argue",
    "evaluate",
    "philosophy",
    "architecture",
    "research",
    "concept",
];
const MATH_KEYWORDS: &[&str] = &[
    "calculate",
    "solve",
    "equation",
    "integral",
    "derivative",
    "sum",
    "probability",
    "statistics",
    "matrix",
    "proof",
];
const VISION_KEYWORDS: &[&str] = &[
    "image",
    "screenshot",
    "diagram",
    "photo",
    "visual",
    "picture",
    "ocr",
];
/// Samples at which measured evidence is ~50% trusted in the router's blend
/// (`n / (n + MIN_SAMPLES)`). Below this, advertised profiles dominate.
const MIN_SAMPLES: u64 = 8;
fn axis_score(question: &str, keywords: &[&str]) -> f32 {
    let q = question.to_ascii_lowercase();
    keywords.iter().filter(|k| q.contains(**k)).count() as f32
}

/// Route a [`ReasoningRequest`] to the best-suited expert.
///
/// For each provider, compute the weighted fit of the question's axis strengths
/// against the provider's profile, then take the best. Ties fall to the first
/// registered (deterministic). This is the "chief engineer picking the
/// specialist" — the *caller* (planner/executive) still owns whether it can
/// answer itself first.
///
/// The public surface is **request-driven**: `best(&ReasoningRequest)` /
/// `consult(&ReasoningRequest)`. Provider identities never appear in call
/// sites except inside a deliberate [`RoutingPolicy::Prefer`] /
/// [`RoutingPolicy::Force`]. There are no target-provider overloads —
/// `routing` lives in the request so new strategies extend the request, not
/// the method list.
///
/// **Routing precedence (one contract):**
///
/// ```text
/// 1. requires      → HARD CONSTRAINT  (exclusion before ranking)
/// 2. routing       → POLICY           (Force pins one; Prefer promotes)
/// 3. score/latency → OPTIMIZATION     (base ordering among survivors)
/// ```
///
/// `requires: "vision"` + `routing: Prefer(Grok)` means *"I need vision.
/// Prefer Grok. If Grok fails, use another vision-capable provider"* — a
/// target lacking a required capability is **excluded with the rest**
/// (preference can never bypass a hard constraint), and `Prefer` never means
/// "the only legal provider" (failover is always bounded by rank order).
/// `routing: Force(Grok)` is the deliberate exception: that expert or an
/// error, no substitution.
///
/// Every pair of these concepts is tested in isolation and all three together
/// (see `preference_beats_score_within_capable_set`,
/// `target_cannot_bypass_the_hard_gate`, `target_and_gate_agree_prefer_the_target`).
///
/// Registration is also defensively validated: duplicate identities collapse
/// to the first registered (measured evidence is keyed by identity, so a
/// second provider under an existing identity would silently merge into — and
/// compete against — the first). Registering the same identity twice is
/// tolerated, never rewarded.
pub struct ExpertRouter {
    providers: Vec<Box<dyn ReasoningProvider>>,
    /// Measured evidence store (optional). When attached, [`Self::fit`] blends
    /// advertised profiles toward the measurements as samples accumulate.
    telemetry: Option<Arc<ProviderTelemetry>>,
}

impl ExpertRouter {
    pub fn new(providers: Vec<Box<dyn ReasoningProvider>>) -> Self {
        Self {
            providers,
            telemetry: None,
        }
    }

    /// Attach measured provider evidence. Routing then blends advertised
    /// profiles toward the measurements as sample counts grow — beliefs set
    /// the ceiling, evidence validates or demotes.
    pub fn with_telemetry(mut self, telemetry: Arc<ProviderTelemetry>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// The default router: ChatGPT (deep reasoning) + Grok (realtime) +
    /// Gemini (vision), each over its dedicated browser adapter on the shared
    /// profile. Deployments wire additional/other providers.
    pub fn default_browser(profile_dir: std::path::PathBuf) -> Self {
        Self::new(vec![
            Box::new(BrowserReasoningProvider::chatgpt(profile_dir.clone())),
            Box::new(BrowserReasoningProvider::grok(profile_dir.clone())),
            Box::new(BrowserReasoningProvider::gemini(profile_dir)),
        ])
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// The weighted fit of a provider's profile against the question's axes
    /// (including the deep-reasoning bias on bare questions). Single source of
    /// truth for both [`Self::best`] and [`Self::consult`] ranking.
    fn fit(&self, question: &str, provider: &dyn ReasoningProvider) -> f32 {
        let belief = {
            let q = question.to_ascii_lowercase();
            let coding = axis_score(&q, CODE_KEYWORDS);
            let realtime = axis_score(&q, REALTIME_KEYWORDS);
            let deep = axis_score(&q, DEEP_KEYWORDS) + 0.2;
            let math = axis_score(&q, MATH_KEYWORDS);
            let vision = axis_score(&q, VISION_KEYWORDS);
            let prof = provider.profile();
            coding * prof.coding
                + realtime * prof.realtime
                + deep * prof.deep_reasoning
                + math * prof.math
                + vision * prof.vision
        };
        self.blend(belief, provider)
    }

    /// Evidence multiplier from a provider's measurements: reliability
    /// (success rate) discounted for slow responses. Belief profiles stay the
    /// ceiling — evidence validates or demotes, never inflates beyond what was
    /// advertised.
    fn evidence_multiplier(stats: &ProviderStats) -> f32 {
        if stats.attempts == 0 {
            return 1.0;
        }
        let reliability = stats.success_rate();
        let latency_s = stats.avg_latency_ms() as f32 / 1000.0;
        // Every second of average latency above 5s shaves 6% (max 25%).
        let penalty = ((latency_s - 5.0).max(0.0) * 0.06).min(0.25);
        (reliability * (1.0 - penalty)).clamp(0.0, 1.0)
    }

    /// Blend the belief fit toward measured evidence as sample counts grow.
    /// With no telemetry, no measurements, or no samples the belief stands
    /// untouched — zero evidence changes nothing.
    fn blend(&self, belief: f32, provider: &dyn ReasoningProvider) -> f32 {
        let Some(telemetry) = &self.telemetry else {
            return belief;
        };
        let Some(stats) = telemetry.stats(provider.provider_id()) else {
            return belief;
        };
        if stats.attempts == 0 {
            return belief;
        }
        let conf = stats.confidence(MIN_SAMPLES);
        let evidence = Self::evidence_multiplier(&stats);
        belief * (1.0 - conf) + belief * evidence * conf
    }

    /// The single ordering score for a candidate under a request. Base is the
    /// policy objective (`Automatic`/`Prefer` → evidence-blended profile fit,
    /// `Fastest` → measured fastness), plus bounded preference boosts for
    /// `latency: Fast` and `confidence: High`. One path for both [`Self::best`]
    /// and [`Self::consult`], so they can never diverge.
    fn order_score(&self, provider: &dyn ReasoningProvider, request: &ReasoningRequest) -> f32 {
        let base = match request.routing {
            RoutingPolicy::Fastest => self.fastness(provider),
            _ => self.fit(&request.question, provider),
        };
        let mut score = base;
        if request.latency == LatencyTier::Fast {
            score += self.latency_boost(provider);
        }
        if request.confidence == ConfidenceTier::High {
            score += self.confidence_boost(provider);
        }
        score
    }

    /// Measured speed on a 0..=1 scale for the [`RoutingPolicy::Fastest`]
    /// objective. Unmeasured providers score 0.0 — evidence ranks above
    /// advertising; a provider with no samples can't claim speed.
    fn fastness(&self, provider: &dyn ReasoningProvider) -> f32 {
        let Some(stats) = self.evidence(provider) else {
            return 0.0;
        };
        // 0s → 1.0; 5s → ~0.17; 30s → ~0.03.
        1.0 / (1.0 + stats.avg_latency_ms() as f32 / 1000.0)
    }

    /// Bounded preference boost for [`LatencyTier::Fast`] (max +0.2): slower
    /// measured providers get less, unmeasured providers get none (no claims
    /// without evidence). Unlike the evidence blend — which only converges as
    /// samples accumulate — this preference applies at full strength from the
    /// first measurement: the planner asked for speed, so speed counts now.
    fn latency_boost(&self, provider: &dyn ReasoningProvider) -> f32 {
        let Some(stats) = self.evidence(provider) else {
            return 0.0;
        };
        if stats.attempts == 0 {
            return 0.0;
        }
        0.2 * (1.0 - (stats.avg_latency_ms() as f32 / 5000.0).min(1.0))
    }

    /// Bounded preference boost for [`ConfidenceTier::High`] (max +0.2): the
    /// provider's measured success rate, at full preference strength once any
    /// evidence exists. Unproven providers get none. Same rationale as
    /// [`Self::latency_boost`] — an explicit preference the planner asked for
    /// takes effect immediately, not only after the blend converges.
    fn confidence_boost(&self, provider: &dyn ReasoningProvider) -> f32 {
        let Some(stats) = self.evidence(provider) else {
            return 0.0;
        };
        if stats.attempts == 0 {
            return 0.0;
        }
        0.2 * stats.success_rate()
    }

    /// The attached telemetry's stats for a provider, if any.
    fn evidence(&self, provider: &dyn ReasoningProvider) -> Option<ProviderStats> {
        self.telemetry
            .as_ref()
            .and_then(|t| t.stats(provider.provider_id()))
    }

    /// Rank the candidates for `request`, best first. This is the one place
    /// the precedence contract is implemented:
    ///
    /// ```text
    /// 1. requires  → HARD CONSTRAINT  (exclusion before scoring)
    /// 2. routing   → POLICY           (Force pins one; Prefer promotes)
    /// 3. score     → OPTIMIZATION     (evidence-blended fit, or fastness for
    ///                                 Fastest, + latency/confidence boosts)
    /// ```
    ///
    /// A `Force` target that is unregistered or fails the gate errors (an
    /// unsatisfiable ask must never silently become something else); a
    /// `Prefer` target excluded by the gate is never re-added, and a promoted
    /// target still fails over to the next-ranked survivors. Reserved policies
    /// (`Consensus`/`Cheapest`) error loudly until they ship.
    fn ranked<'a>(
        &'a self,
        request: &ReasoningRequest,
    ) -> Result<Vec<&'a dyn ReasoningProvider>, String> {
        match request.routing {
            RoutingPolicy::Consensus => {
                return Err(
                    "routing policy 'consensus' is reserved — multi-expert combination \
                     is not built yet (no silent fallback)"
                        .to_string(),
                )
            }
            RoutingPolicy::Cheapest => {
                return Err(
                    "routing policy 'cheapest' is reserved — cost telemetry is not \
                     collected yet (no silent fallback)"
                        .to_string(),
                )
            }
            _ => {}
        }
        if let RoutingPolicy::Force(id) = request.routing {
            let Some(provider) = self
                .providers
                .iter()
                .map(|p| p.as_ref())
                .find(|p| p.provider_id() == id)
            else {
                return Err(format!("forced provider '{}' is not registered", id.name()));
            };
            if !provider.capabilities().has_all(&request.requires) {
                let names: Vec<&str> = request.requires.iter().map(|c| c.name()).collect();
                return Err(format!(
                    "forced provider '{}' lacks required capabilities: {}",
                    id.name(),
                    names.join(", ")
                ));
            }
            return Ok(vec![provider]);
        }
        // Eligible survivors, collapsed to one entry per identity: evidence is
        // keyed by identity, so a duplicate registration would otherwise
        // silently compete against (and merge telemetry into) the first.
        // First registered wins — the same deterministic tie rule `best`
        // documents.
        let mut seen: Vec<ProviderId> = Vec::new();
        let mut ranked: Vec<&dyn ReasoningProvider> = Vec::new();
        for provider in self.providers.iter().map(|p| p.as_ref()) {
            if !provider.capabilities().has_all(&request.requires) {
                continue;
            }
            if seen.contains(&provider.provider_id()) {
                continue;
            }
            seen.push(provider.provider_id());
            ranked.push(provider);
        }
        ranked.sort_by(|a, b| {
            self.order_score(*b, request)
                .partial_cmp(&self.order_score(*a, request))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let RoutingPolicy::Prefer(id) = request.routing {
            if let Some(pos) = ranked.iter().position(|p| p.provider_id() == id) {
                let provider = ranked.remove(pos);
                ranked.insert(0, provider);
            }
        }
        Ok(ranked)
    }

    /// The single best expert for this request. `None` when the router has no
    /// providers or the request cannot be ranked (reserved policy, forced but
    /// unregistered provider). Ties fall to the first registered
    /// (deterministic). A synchronous preview — [`Self::consult`] is the
    /// authoritative path (it validates and executes).
    pub fn best(&self, request: &ReasoningRequest) -> Option<&dyn ReasoningProvider> {
        self.ranked(request).ok().and_then(|v| v.into_iter().next())
    }

    /// Ask the best expert; on failure fall through to the next-best expert.
    /// Returns the first successful reply (a `ConsultResult` that records who
    /// answered). Errors when the request cannot be honored (reserved policy,
    /// forced-but-unregistered or gate-failing provider, no capable provider)
    /// or when every eligible expert fails.
    pub async fn consult(&self, request: &ReasoningRequest) -> Result<ConsultResult, String> {
        if self.providers.is_empty() {
            return Err("no reasoning providers registered".to_string());
        }
        // Rank once, then try in order — failover is bounded by the provider
        // count (a `Force` request ranks exactly one), and each provider is
        // best-effort by design.
        let ranked = self.ranked(request)?;
        if ranked.is_empty() {
            let names: Vec<&str> = request.requires.iter().map(|c| c.name()).collect();
            return Err(format!(
                "no registered provider supports required capabilities: {}",
                names.join(", ")
            ));
        }

        let mut last_err = String::new();
        for provider in ranked {
            let started = std::time::Instant::now();
            match provider.reason(&request.question).await {
                Ok(text) => {
                    self.record(provider.provider_id(), true, started, None);
                    return Ok(ConsultResult {
                        text,
                        provider: provider.id().to_string(),
                        transport: provider.transport(),
                        profile: provider.profile(),
                        capabilities: provider.capabilities(),
                        measured: self
                            .telemetry
                            .as_ref()
                            .and_then(|t| t.stats(provider.provider_id())),
                    });
                }
                Err(e) => {
                    self.record(provider.provider_id(), false, started, Some(&e));
                    last_err = e;
                }
            }
        }
        Err(format!("every reasoning expert failed: {last_err}"))
    }

    /// Record a consult outcome — success for the winner, failure per skipped
    /// attempt — on the attached telemetry, if any.
    fn record(
        &self,
        provider: ProviderId,
        success: bool,
        started: std::time::Instant,
        error: Option<&str>,
    ) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.record(MeasuredOutcome {
                provider,
                success,
                duration_ms: started.elapsed().as_millis() as u64,
                error: error.map(|e| e.chars().take(200).collect()),
                at_ms: now_ms(),
            });
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ───────────────────────────────────────────────────────────────────────────
// ReasoningAdapter — the planner-visible seam
// ───────────────────────────────────────────────────────────────────────────

/// Planner-visible adapter over the expert router.
///
/// Exposes the layer to goal-execution as `reasoning.consult` — an evidence
/// action "consult an external intelligence" routes here, the router picks the
/// best-suited expert, and the reply (plus which expert answered) comes back.
///
/// Planner-path defaults: `question` falls back to the `query` param (the
/// planner always sets it to the goal). The router is built from the same
/// dedicated profile the WebAgent uses.
pub struct ReasoningAdapter {
    router: ExpertRouter,
    /// The evidence surface behind `reasoning.stats`. Shares its `Arc` with
    /// the router when built by [`Self::new`], so what the router records is
    /// exactly what the action reads.
    telemetry: Option<Arc<ProviderTelemetry>>,
}

impl Default for ReasoningAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReasoningAdapter {
    /// The default adapter: the three shipped browser experts over the
    /// dedicated profile, plus measured provider evidence persisted under
    /// `~/.friday/telemetry/` so successes/latencies accumulate across runs.
    pub fn new() -> Self {
        let profile = crate::adapters::webagent::browser_profile_dir();
        let telemetry = Arc::new(ProviderTelemetry::with_persist(
            crate::adapters::telemetry::default_telemetry_path(),
        ));
        Self {
            router: ExpertRouter::default_browser(profile).with_telemetry(Arc::clone(&telemetry)),
            telemetry: Some(telemetry),
        }
    }

    /// Replace the router (tests, or a deployment wiring API/local providers).
    pub fn with_router(router: ExpertRouter) -> Self {
        Self {
            router,
            telemetry: None,
        }
    }

    /// Attach measured evidence to the adapter (the `reasoning.stats` surface).
    /// The router records through its own telemetry — share the same `Arc`
    /// when both are needed.
    pub fn with_telemetry(mut self, telemetry: Arc<ProviderTelemetry>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    fn resolve_question(&self, req: &ActionRequest) -> Option<String> {
        let q = req
            .parameters
            .get("question")
            .and_then(|v| v.as_str())
            .or_else(|| req.parameters.get("query").and_then(|v| v.as_str()));
        q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Resolve an optional target identity from the `provider` param (e.g.
    /// `"grok"`). The identity feeds the routing policy: `prefer` promotes it,
    /// `force` pins it. Unknown names and `generic` mean "no constraint" — the
    /// router falls back to profile-based selection, exactly like the webagent
    /// adapter's tolerated labels.
    fn resolve_target(&self, req: &ActionRequest) -> Option<ProviderId> {
        req.parameters
            .get("provider")
            .and_then(|v| v.as_str())
            .and_then(ProviderId::from_name)
            .filter(|id| !matches!(id, ProviderId::Generic))
    }

    /// Resolve required capabilities from the `requires` param — comma or
    /// space separated names (`"vision,citations"`). Unknown names error
    /// loudly: a requirement that can never be satisfied must not silently
    /// fall back to a provider that lacks it.
    fn resolve_required(&self, req: &ActionRequest) -> Result<Vec<ProviderCapability>, String> {
        let Some(raw) = req.parameters.get("requires").and_then(|v| v.as_str()) else {
            return Ok(Vec::new());
        };
        let mut caps = Vec::new();
        for part in raw
            .split([',', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match ProviderCapability::from_name(part) {
                Some(c) => caps.push(c),
                None => {
                    return Err(format!(
                        "reasoning.consult: unknown required capability: {part}"
                    ))
                }
            }
        }
        Ok(caps)
    }

    /// Resolve the routing policy. An explicit `routing` param names it
    /// (`automatic`, `prefer`, `force`, `fastest`, `consensus`, `cheapest`);
    /// without one, a `provider` param implies [`RoutingPolicy::Prefer`] (the
    /// historic "promote the target, fail over after it" contract) and
    /// otherwise the request is [`RoutingPolicy::Automatic`]. `force` without a
    /// provider is an error — forcing nothing is an unsatisfiable ask — while
    /// `prefer` without a provider tolerates down to `automatic` (nothing to
    /// promote). Unknown policy names error loudly, like unknown required
    /// capabilities.
    fn resolve_routing(
        &self,
        req: &ActionRequest,
        target: Option<ProviderId>,
    ) -> Result<RoutingPolicy, String> {
        let name = req
            .parameters
            .get("routing")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase());
        let policy = match name.as_deref() {
            None => target.map_or(RoutingPolicy::Automatic, RoutingPolicy::Prefer),
            Some("automatic") => RoutingPolicy::Automatic,
            Some("prefer") => target.map_or(RoutingPolicy::Automatic, RoutingPolicy::Prefer),
            Some("force") => {
                let id = target.ok_or_else(|| {
                    "reasoning.consult: routing 'force' requires a 'provider' param".to_string()
                })?;
                RoutingPolicy::Force(id)
            }
            Some("fastest") => RoutingPolicy::Fastest,
            Some("consensus") => RoutingPolicy::Consensus,
            Some("cheapest") => RoutingPolicy::Cheapest,
            Some(other) => {
                return Err(format!(
                    "reasoning.consult: unknown routing policy: {other}"
                ))
            }
        };
        Ok(policy)
    }

    /// Resolve the latency preference tier. Unknown values are tolerated as
    /// [`LatencyTier::Any`] — a preference, not a gate.
    fn resolve_latency(&self, req: &ActionRequest) -> LatencyTier {
        match req
            .parameters
            .get("latency")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("fast") => LatencyTier::Fast,
            _ => LatencyTier::Any,
        }
    }

    /// Resolve the confidence preference tier. Unknown values are tolerated as
    /// [`ConfidenceTier::Any`] — a preference, not a gate.
    fn resolve_confidence(&self, req: &ActionRequest) -> ConfidenceTier {
        match req
            .parameters
            .get("confidence")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("high") => ConfidenceTier::High,
            _ => ConfidenceTier::Any,
        }
    }

    async fn consult(&self, req: &ActionRequest) -> ActionResult {
        let Some(question) = self.resolve_question(req) else {
            return ActionResult::error(
                "reasoning.consult requires a question (param 'question' or 'query')",
            );
        };
        let target = self.resolve_target(req);
        let routing = match self.resolve_routing(req, target) {
            Ok(r) => r,
            Err(e) => return ActionResult::error(e),
        };
        let required = match self.resolve_required(req) {
            Ok(r) => r,
            Err(e) => return ActionResult::error(e),
        };
        let request = ReasoningRequest::new(question)
            .with_routing(routing)
            .requires(&required)
            .with_latency(self.resolve_latency(req))
            .with_confidence(self.resolve_confidence(req));
        match self.router.consult(&request).await {
            Ok(r) => ActionResult::success(json!({
                "text": r.text,
                "provider": r.provider,
                "transport": r.transport.label(),
                "measured": r.measured,
                "routing": routing.name(),
                "targeted": routing.target().map(|t| t.name()),
                "required": required.iter().map(|c| c.name()).collect::<Vec<_>>(),
                "capabilities": r.capabilities.enabled(),
            })),
            Err(e) => ActionResult::error(e),
        }
    }

    /// The measured evidence table: per-provider success rate, average
    /// latency, and last-seen — the facts the router blends against.
    async fn stats(&self) -> ActionResult {
        match &self.telemetry {
            Some(t) => ActionResult::success(json!({ "providers": t.snapshot() })),
            None => ActionResult::error("no provider telemetry attached".to_string()),
        }
    }
}

#[async_trait]
impl ActionAdapter for ReasoningAdapter {
    fn tool(&self) -> &str {
        "reasoning"
    }

    fn actions(&self) -> Vec<&str> {
        vec!["consult", "stats"]
    }

    async fn execute(&self, req: ActionRequest) -> ActionResult {
        match req.action.as_str() {
            "consult" => self.consult(&req).await,
            "stats" => self.stats().await,
            other => ActionResult::error(format!("unknown reasoning action: {other}")),
        }
    }
}
/// Capability records for the reasoning verbs. `consult` asks an external
/// intelligence — it never types, clicks, or navigates (`Risk::Medium`, like
/// `web.type`: a live site interaction, but only a question).
pub fn reasoning_capability_specs(
) -> Vec<friday_planner::execution::capability_registry::CapabilitySpec> {
    use friday_planner::execution::capability_registry::{CapabilitySpec, Risk};
    vec![
        CapabilitySpec::native(
            "reasoning.consult".to_string(),
            "Consult an external intelligence".to_string(),
            "Ask the best-suited external expert (browser/API/local) a question and return its \\\
             reply. Params: question (default: the goal), routing (policy: automatic (default) \\\
             | prefer | force | fastest; consensus/cheapest are reserved and error until they \\\
             ship), provider (target identity for prefer/force, e.g. \"grok\" — a bare provider \\\
             param implies prefer; profile-based selection when absent or unknown), requires \\\
             (comma-separated capability names to gate on, e.g. \"vision,citations\" — providers \\\
             lacking any are excluded before ranking), latency (any|fast — prefer the fastest \\\
             measured expert), confidence (any|high — prefer the most proven expert). Forcing a \\\
             provider is the deliberate override (user asked, A/B, debugging, benchmarking) — \\\
             automatic routing is the default."
                .to_string(),
            "reasoning".to_string(),
            "consult".to_string(),
            Risk::Medium,
        ),
        CapabilitySpec::native(
            "reasoning.stats".to_string(),
            "Read measured provider evidence".to_string(),
            "Return the measured success/latency table for every consulted expert — the \\\
             evidence the router blends against advertised profiles."
                .to_string(),
            "reasoning".to_string(),
            "stats".to_string(),
            Risk::Safe,
        ),
    ]
}

/// Register the reasoning verbs into the planner's capability registry. Call
/// alongside registering a [`ReasoningAdapter`] with the kernel/evidence seam.
pub fn register_reasoning_capabilities(
    registry: &friday_planner::execution::capability_registry::CapabilityRegistry,
) {
    for spec in reasoning_capability_specs() {
        registry.register(spec);
    }
}

#[cfg(test)]
mod tests {
    // Live-browser tests intentionally hold a std Mutex across await to serialize
    // real Chrome access; serialization is the point, not a bug.
    #![allow(clippy::await_holding_lock)]
    use super::*;

    /// A fake provider with a fixed profile and canned reply — proves routing
    /// and failover without any browser. Each fake advertises a distinct
    /// canonical identity so measured evidence keys independently.
    struct FakeProvider {
        id: &'static str,
        profile: ExpertProfile,
        fail: bool,
        provider_id: ProviderId,
        /// Optional capability override (defaults to the identity's). Tests
        /// use it to simulate a qualification that flipped a flag — e.g. a
        /// deployment that measured Grok's citations and promoted the belief.
        caps: Option<ProviderCapabilities>,
    }

    #[async_trait]
    impl ReasoningProvider for FakeProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn transport(&self) -> Transport {
            Transport::Api
        }
        fn profile(&self) -> ExpertProfile {
            self.profile
        }
        fn provider_id(&self) -> ProviderId {
            self.provider_id
        }
        fn capabilities(&self) -> ProviderCapabilities {
            self.caps
                .unwrap_or_else(|| self.provider_id.default_capabilities())
        }
        async fn reason(&self, _question: &str) -> Result<String, String> {
            if self.fail {
                Err(format!("{} failed", self.id))
            } else {
                Ok(format!("answer from {}", self.id))
            }
        }
    }

    fn code_expert() -> FakeProvider {
        FakeProvider {
            id: "code-expert",
            profile: ExpertProfile::new(0.95, 0.2, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::ChatGPT,
            caps: None,
        }
    }

    fn deep_expert() -> FakeProvider {
        FakeProvider {
            id: "deep-expert",
            profile: ExpertProfile::new(0.4, 0.2, 0.95, 0.5, 0.1),
            fail: false,
            provider_id: ProviderId::Claude,
            caps: None,
        }
    }

    fn realtime_expert() -> FakeProvider {
        FakeProvider {
            id: "realtime-expert",
            profile: ExpertProfile::new(0.2, 0.95, 0.3, 0.2, 0.1),
            fail: false,
            provider_id: ProviderId::Grok,
            caps: None,
        }
    }

    fn local_expert() -> FakeProvider {
        FakeProvider {
            id: "local-expert",
            profile: ExpertProfile::new(0.9, 0.1, 0.5, 0.7, 0.1),
            fail: false,
            provider_id: ProviderId::Local,
            caps: None,
        }
    }

    #[tokio::test]
    async fn routes_coding_questions_to_the_code_expert() {
        let router = ExpertRouter::new(vec![
            Box::new(deep_expert()),
            Box::new(code_expert()),
            Box::new(realtime_expert()),
        ]);
        let best = router
            .best(&ReasoningRequest::new(
                "why does this rust function panic on compile?",
            ))
            .unwrap();
        assert_eq!(best.id(), "code-expert");
    }

    #[tokio::test]
    async fn routes_deep_questions_to_the_deep_expert() {
        let router = ExpertRouter::new(vec![
            Box::new(realtime_expert()),
            Box::new(code_expert()),
            Box::new(deep_expert()),
        ]);
        let best = router
            .best(&ReasoningRequest::new(
                "analyze and explain the philosophical arguments for determinism",
            ))
            .unwrap();
        assert_eq!(best.id(), "deep-expert");
    }

    #[tokio::test]
    async fn routes_realtime_questions_to_the_realtime_expert() {
        let router = ExpertRouter::new(vec![
            Box::new(deep_expert()),
            Box::new(code_expert()),
            Box::new(realtime_expert()),
        ]);
        let best = router
            .best(&ReasoningRequest::new(
                "what is the latest news and today's score?",
            ))
            .unwrap();
        assert_eq!(best.id(), "realtime-expert");
    }

    #[tokio::test]
    async fn consult_returns_the_winning_expert_and_reply() {
        let router = ExpertRouter::new(vec![Box::new(realtime_expert()), Box::new(code_expert())]);
        let r = router
            .consult(&ReasoningRequest::new("how do I debug this rust crash?"))
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert");
        assert_eq!(r.text, "answer from code-expert");
        assert_eq!(r.transport, Transport::Api);
    }

    #[tokio::test]
    async fn consult_fails_over_to_the_next_best_expert() {
        let mut failing = code_expert();
        failing.fail = true;
        let router = ExpertRouter::new(vec![Box::new(failing), Box::new(deep_expert())]);
        // Best fit is the failing code expert; consult must fall through.
        let r = router
            .consult(&ReasoningRequest::new("debug this rust panic"))
            .await
            .unwrap();
        assert_eq!(r.provider, "deep-expert");
    }

    // ── Routing policy: Prefer / Force ───────────────────────────────

    /// `Prefer` promotes the target above the profile order but failover still
    /// proceeds in rank order after it — "I need vision. Prefer Grok. If Grok
    /// fails, another vision-capable provider".
    #[tokio::test]
    async fn prefer_promotes_the_requested_expert() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        // By profile, a coding question goes to the code expert...
        assert_eq!(
            router
                .best(&ReasoningRequest::new("debug this rust crash"))
                .unwrap()
                .id(),
            "code-expert"
        );
        // ...but Prefer(Grok) promotes the realtime expert to the front.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust crash")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Grok)),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "realtime-expert");
        assert_eq!(r.text, "answer from realtime-expert");
    }

    /// An unknown `Prefer` target is ignored — profile selection wins (the
    /// tolerated-label rule: an unknown provider is a *preference*, not a gate).
    #[tokio::test]
    async fn prefer_unknown_target_falls_back_to_profile() {
        let router = ExpertRouter::new(vec![Box::new(deep_expert()), Box::new(code_expert())]);
        // Perplexity is not registered — the preference is ignored, profile wins.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Perplexity)),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert");
    }

    #[tokio::test]
    async fn best_promotes_a_preferred_identity() {
        let router = ExpertRouter::new(vec![Box::new(deep_expert()), Box::new(code_expert())]);
        assert_eq!(
            router
                .best(&ReasoningRequest::new("analyze this deep question"))
                .unwrap()
                .id(),
            "deep-expert"
        );
        // Preferring the code identity wins even on a deep question.
        assert_eq!(
            router
                .best(
                    &ReasoningRequest::new("analyze this deep question")
                        .with_routing(RoutingPolicy::Prefer(ProviderId::ChatGPT))
                )
                .unwrap()
                .id(),
            "code-expert"
        );
        // No preference = plain best.
        assert_eq!(
            router
                .best(&ReasoningRequest::new("analyze this deep question"))
                .unwrap()
                .id(),
            "deep-expert"
        );
    }

    /// `Force` pins the candidate set to one identity — the deliberate
    /// override (user asked, A/B test, debugging, benchmarking). No failover:
    /// a forced answer is the point.
    #[tokio::test]
    async fn force_pins_the_requested_expert() {
        let mut failing_code = code_expert();
        failing_code.fail = true;
        let router = ExpertRouter::new(vec![
            Box::new(failing_code),
            Box::new(realtime_expert()),
            Box::new(deep_expert()),
        ]);
        // The best-fit expert is failing, but Force(Grok) pins Grok — and does
        // NOT fail over to the deep expert.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Force(ProviderId::Grok)),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "realtime-expert");
        assert_eq!(r.text, "answer from realtime-expert");
    }

    /// Forcing an unregistered identity errors loudly — an unsatisfiable ask
    /// must never silently become something else.
    #[tokio::test]
    async fn force_unregistered_identity_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let err = router
            .consult(
                &ReasoningRequest::new("hi")
                    .with_routing(RoutingPolicy::Force(ProviderId::Perplexity)),
            )
            .await
            .unwrap_err();
        assert!(
            err.contains("forced provider 'perplexity' is not registered"),
            "err: {err}"
        );
    }

    /// A forced provider that fails is an error, never a failover.
    #[tokio::test]
    async fn force_never_substitutes_on_failure() {
        let mut failing = realtime_expert();
        failing.fail = true;
        let router = ExpertRouter::new(vec![Box::new(failing), Box::new(code_expert())]);
        let err = router
            .consult(
                &ReasoningRequest::new("debug this rust crash")
                    .with_routing(RoutingPolicy::Force(ProviderId::Grok)),
            )
            .await
            .unwrap_err();
        assert!(
            err.contains("every reasoning expert failed"),
            "a forced expert must not silently fall through to another: {err}"
        );
    }

    /// The hard gate still binds under Force: a forced provider lacking a
    /// required capability errors instead of being offered the question.
    #[tokio::test]
    async fn force_cannot_bypass_the_hard_gate() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(local_expert())]);
        let err = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Force(ProviderId::ChatGPT))
                    .requires(&[ProviderCapability::Offline]),
            )
            .await
            .unwrap_err();
        assert!(
            err.contains("forced provider 'chatgpt' lacks required capabilities: offline"),
            "err: {err}"
        );
    }

    #[tokio::test]
    async fn adapter_consult_provider_param_prefers_the_expert() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash", "provider": "grok" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["provider"], "realtime-expert");
        assert_eq!(res.output["routing"], "prefer");
        assert_eq!(res.output["targeted"], "grok");
    }

    #[tokio::test]
    async fn adapter_consult_routing_force_param_pins_the_expert() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash", "routing": "force", "provider": "grok" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["provider"], "realtime-expert");
        assert_eq!(res.output["routing"], "force");
        assert_eq!(res.output["targeted"], "grok");
    }

    #[tokio::test]
    async fn adapter_consult_routing_force_without_provider_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "hi", "routing": "force" }),
            ))
            .await;
        assert!(!res.success);
        assert!(
            res.error
                .unwrap()
                .contains("routing 'force' requires a 'provider'"),
            "forcing nothing is an unsatisfiable ask"
        );
    }

    #[tokio::test]
    async fn adapter_consult_unknown_routing_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "hi", "routing": "teleport" }),
            ))
            .await;
        assert!(!res.success);
        assert!(
            res.error
                .unwrap()
                .contains("unknown routing policy: teleport"),
            "an unknown policy is a config error, never silently automatic"
        );
    }

    #[tokio::test]
    async fn adapter_consult_routing_automatic_ignores_a_provider_param() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        // Explicit `routing: automatic` means the provider param is deliberately
        // ignored — the router decides by profile fit.
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash", "routing": "automatic", "provider": "grok" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["provider"], "code-expert");
        assert_eq!(res.output["routing"], "automatic");
        assert!(res.output["targeted"].is_null());
    }

    // ── Required capabilities (the hard gate) ─────────────────────────

    #[tokio::test]
    async fn consult_requires_gates_to_capable_providers() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(local_expert())]);
        // By score, the code expert (ChatGPT) wins a coding question...
        assert_eq!(
            router
                .best(&ReasoningRequest::new("debug this rust panic"))
                .unwrap()
                .id(),
            "code-expert"
        );
        // ...but the offline requirement excludes it — only Local survives.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .requires(&[ProviderCapability::Offline]),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "local-expert");
        assert!(r.capabilities.supports(ProviderCapability::Offline));
    }

    #[tokio::test]
    async fn consult_requires_errors_when_no_provider_qualifies() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        let err = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .requires(&[ProviderCapability::Offline]),
            )
            .await
            .unwrap_err();
        assert!(
            err.contains("no registered provider supports required capabilities: offline"),
            "err: {err}"
        );
    }

    #[tokio::test]
    async fn adapter_consult_requires_param_gates_the_expert() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(local_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash", "requires": "offline" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["provider"], "local-expert");
        assert_eq!(res.output["required"][0], "offline");
        let caps = res.output["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert!(caps.iter().any(|c| c == "offline"));
    }

    #[tokio::test]
    async fn adapter_consult_unknown_required_capability_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "hi", "requires": "teleport" }),
            ))
            .await;
        assert!(!res.success);
        assert!(
            res.error
                .unwrap()
                .contains("unknown required capability: teleport"),
            "an unsatisfiable requirement must fail loudly, never silently"
        );
    }

    #[tokio::test]
    async fn adapter_consult_unknown_provider_is_tolerated() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(realtime_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        // An unknown label must not break the consult — profile selection wins.
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash", "provider": "not-a-provider" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["provider"], "code-expert");
        assert!(res.output["targeted"].is_null(), "no constraint recorded");
    }

    // ── Routing precedence: requires > policy > score ──────────────────

    /// The full three-concept contract: a preferred target is EXCLUDED when it
    /// lacks a required capability — preference never bypasses a hard
    /// constraint. `Prefer(ChatGPT)` + `requires: offline` must land on Local,
    /// not the requested (incapable) code expert.
    #[tokio::test]
    async fn target_cannot_bypass_the_hard_gate() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(local_expert())]);
        // By score, the code expert wins a coding question...
        assert_eq!(
            router
                .best(&ReasoningRequest::new("debug this rust panic"))
                .unwrap()
                .id(),
            "code-expert"
        );
        // ...but the offline requirement excludes it even though it is the
        // preferred target — only Local survives the gate.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::ChatGPT))
                    .requires(&[ProviderCapability::Offline]),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "local-expert");
        assert!(r.capabilities.supports(ProviderCapability::Offline));
    }

    /// When the target and the gate agree, the target wins — preference and
    /// constraint point the same direction.
    #[tokio::test]
    async fn target_and_gate_agree_prefer_the_target() {
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(local_expert())]);
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Local))
                    .requires(&[ProviderCapability::Offline]),
            )
            .await
            .unwrap();
        assert_eq!(r.provider, "local-expert");
    }
    /// Among the capable survivors, preference beats score: on a coding
    /// question, preferring Grok wins even though Claude fits the coding axes
    /// better — `Prefer` is a *target*, not a veto on ranking. Grok's
    /// citations flag here is a qualification override (identity defaults
    /// don't claim it) — which also proves a measured flag flip changes
    /// routing.
    #[tokio::test]
    async fn preference_beats_score_within_capable_set() {
        // Grok with a qualification override: citations measured → promoted.
        let mut grok = realtime_expert();
        grok.caps = Some(ProviderCapabilities {
            citations: true,
            ..ProviderCapabilities::default()
        });

        let router = ExpertRouter::new(vec![
            Box::new(code_expert()), // ChatGPT — no citations, excluded by gate
            Box::new(deep_expert()), // Claude — citations ✓
            Box::new(grok),          // Grok — citations ✓ (qualified)
        ]);
        // Without a target, the best-fit citations-capable survivor wins
        // (Claude's deep profile out-scores Grok on a coding question).
        let no_target = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .requires(&[ProviderCapability::Citations]),
            )
            .await
            .unwrap();
        assert_eq!(no_target.provider, "deep-expert");
        // Preferring Grok promotes it above the score ordering.
        let targeted = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Grok))
                    .requires(&[ProviderCapability::Citations]),
            )
            .await
            .unwrap();
        assert_eq!(targeted.provider, "realtime-expert");
        // The gate held for both: ChatGPT (no citations) was never in the
        // running, and the winner actually carries the required capability.
        assert!(targeted
            .capabilities
            .supports(ProviderCapability::Citations));
    }

    // ── Measured evidence (provider telemetry) ──────────────────────────

    #[tokio::test]
    async fn evidence_demotes_a_failing_provider() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router = ExpertRouter::new(vec![Box::new(deep_expert()), Box::new(code_expert())])
            .with_telemetry(Arc::clone(&telemetry));

        // Belief routing first: the advertised code expert wins a coding question.
        let best_before = router
            .best(&ReasoningRequest::new("debug this rust panic"))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(best_before, "code-expert");

        // Evidence: 10 failed code consults vs 10 successful deep consults.
        for _ in 0..10 {
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::ChatGPT, // code_expert's identity
                success: false,
                duration_ms: 900,
                error: Some("timeout".into()),
                at_ms: 1,
            });
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::Claude, // deep_expert's identity
                success: true,
                duration_ms: 300,
                error: None,
                at_ms: 1,
            });
        }

        let best_after = router
            .best(&ReasoningRequest::new("debug this rust panic"))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(
            best_after, "deep-expert",
            "measured failures must demote the advertised code expert"
        );
    }

    #[tokio::test]
    async fn consult_records_the_winners_measured_outcome() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router =
            ExpertRouter::new(vec![Box::new(code_expert())]).with_telemetry(Arc::clone(&telemetry));

        let r = router
            .consult(&ReasoningRequest::new("how do I debug this rust crash?"))
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert");
        // The winning provider's measured evidence rides along on the result.
        let measured = r.measured.expect("winner measured");
        assert_eq!(measured.provider, ProviderId::ChatGPT);
        assert_eq!(measured.attempts, 1);
        assert_eq!(measured.successes, 1);
        assert_eq!(measured.last_error, None);
    }

    #[tokio::test]
    async fn consult_records_failures_for_skipped_experts() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let mut failing = code_expert();
        failing.fail = true;
        let router = ExpertRouter::new(vec![Box::new(failing), Box::new(deep_expert())])
            .with_telemetry(Arc::clone(&telemetry));

        let r = router
            .consult(&ReasoningRequest::new("debug this rust panic"))
            .await
            .unwrap();
        assert_eq!(r.provider, "deep-expert");
        // The failed code attempt was recorded; the deep winner succeeded.
        let code_stats = telemetry.stats(ProviderId::ChatGPT).expect("code evidence");
        assert_eq!(code_stats.failures, 1);
        let deep_stats = telemetry.stats(ProviderId::Claude).expect("deep evidence");
        assert_eq!(deep_stats.successes, 1);
    }

    #[tokio::test]
    async fn stats_action_returns_the_measured_table() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router =
            ExpertRouter::new(vec![Box::new(code_expert())]).with_telemetry(Arc::clone(&telemetry));
        let adapter = ReasoningAdapter::with_router(router).with_telemetry(telemetry);

        // One consult creates evidence.
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust crash" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);

        let res = adapter
            .execute(ActionRequest::new("reasoning", "stats", json!({})))
            .await;
        assert!(res.success, "stats failed: {:?}", res.error);
        let providers = res.output["providers"].as_array().expect("providers array");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["provider"], "chatgpt");
        assert_eq!(providers[0]["successes"], 1);
    }

    #[tokio::test]
    async fn consult_errors_when_every_expert_fails() {
        let mut a = code_expert();
        a.fail = true;
        let mut b = deep_expert();
        b.fail = true;
        let router = ExpertRouter::new(vec![Box::new(a), Box::new(b)]);
        let err = router
            .consult(&ReasoningRequest::new("debug this rust panic"))
            .await
            .unwrap_err();
        assert!(err.contains("every reasoning expert failed"), "err: {err}");
    }

    #[tokio::test]
    async fn empty_router_errors() {
        let router = ExpertRouter::new(Vec::new());
        assert!(router.is_empty());
        let err = router
            .consult(&ReasoningRequest::new("anything"))
            .await
            .unwrap_err();
        assert!(err.contains("no reasoning providers"));
    }

    // ── Fastest policy and preference tiers ────────────────────────────

    /// `Fastest` makes measured latency the objective: the fastest measured
    /// expert is first, and unmeasured experts rank after measured ones (no
    /// speed claims without evidence).
    #[tokio::test]
    async fn fastest_policy_orders_by_measured_latency() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router = ExpertRouter::new(vec![
            Box::new(realtime_expert()), // Grok — will be measured slow
            Box::new(code_expert()),     // ChatGPT — will be measured fast
            Box::new(deep_expert()),     // Claude — never measured
        ])
        .with_telemetry(Arc::clone(&telemetry));
        for _ in 0..4 {
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::Grok,
                success: true,
                duration_ms: 6000,
                error: None,
                at_ms: 1,
            });
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::ChatGPT,
                success: true,
                duration_ms: 2000,
                error: None,
                at_ms: 1,
            });
        }
        // By profile, a coding question would pick the code expert anyway; the
        // assertion that matters is the ordering under Fastest, where the fast
        // code expert beats the slow realtime expert and the unmeasured deep
        // expert ranks last.
        let ranked = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Fastest),
            )
            .await
            .unwrap();
        assert_eq!(ranked.provider, "code-expert");
        // The unmeasured provider is never first under Fastest.
        let best = router
            .best(
                &ReasoningRequest::new("analyze this deep question")
                    .with_routing(RoutingPolicy::Fastest),
            )
            .expect("best");
        assert_ne!(best.id(), "deep-expert", "unmeasured experts rank last");
    }

    /// `latency: Fast` is a preference, not a policy: on a *near-tie* it
    /// nudges the faster measured expert ahead, but never excludes anyone.
    /// The advertised profiles here are almost identical — the only material
    /// difference is measured latency.
    #[tokio::test]
    async fn latency_fast_prefers_the_faster_provider() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let slow_expert = FakeProvider {
            id: "slow-expert",
            profile: ExpertProfile::new(0.95, 0.2, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::ChatGPT,
            caps: None,
        };
        let fast_expert = FakeProvider {
            id: "fast-expert",
            profile: ExpertProfile::new(0.90, 0.2, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::Claude,
            caps: None,
        };
        let router = ExpertRouter::new(vec![Box::new(slow_expert), Box::new(fast_expert)])
            .with_telemetry(Arc::clone(&telemetry));
        // One fresh measurement each: 8s for the slow expert, 0.8s for the fast.
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::ChatGPT,
            success: true,
            duration_ms: 8000,
            error: None,
            at_ms: 1,
        });
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::Claude,
            success: true,
            duration_ms: 800,
            error: None,
            at_ms: 1,
        });
        // Near-tie, no preference: the slow expert's slightly stronger profile
        // still wins.
        let without = router
            .best(&ReasoningRequest::new("debug this crash"))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(without, "slow-expert");
        // With the latency preference, the faster measured expert is promoted.
        let with = router
            .best(&ReasoningRequest::new("debug this crash").with_latency(LatencyTier::Fast))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(
            with, "fast-expert",
            "latency: fast must promote the faster measured expert"
        );
    }

    /// `confidence: High` prefers the proven provider: a candidate with a
    /// stronger advertised profile but a single failure loses to the reliable
    /// one with a single success. Again a preference, not a policy.
    #[tokio::test]
    async fn confidence_high_prefers_the_proven_provider() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let advertised_expert = FakeProvider {
            id: "advertised-expert",
            profile: ExpertProfile::new(1.0, 0.2, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::ChatGPT,
            caps: None,
        };
        let proven_expert = FakeProvider {
            id: "proven-expert",
            profile: ExpertProfile::new(0.85, 0.2, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::Claude,
            caps: None,
        };
        let router = ExpertRouter::new(vec![Box::new(advertised_expert), Box::new(proven_expert)])
            .with_telemetry(Arc::clone(&telemetry));
        // One fresh measurement each: one failure, one success.
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::ChatGPT,
            success: false,
            duration_ms: 300,
            error: Some("timeout".into()),
            at_ms: 1,
        });
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::Claude,
            success: true,
            duration_ms: 300,
            error: None,
            at_ms: 1,
        });
        // Near-tie, no preference: the stronger advertised profile still wins
        // despite its single failure (the blend has barely converged).
        let without = router
            .best(&ReasoningRequest::new("debug this crash"))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(without, "advertised-expert");
        // With the confidence preference, the proven provider wins outright.
        let with = router
            .best(&ReasoningRequest::new("debug this crash").with_confidence(ConfidenceTier::High))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(
            with, "proven-expert",
            "confidence: high must prefer the proven expert"
        );
    }

    // ── Adversarial inputs ──────────────────────────────────────────────
    //
    // The router is a scheduler, so it must survive hostile inputs without
    // panicking and without letting a preference override a hard constraint.
    // These tests pin the frozen contract; the scoring model does not change.

    /// The headline adversarial case: an explicit preference NEVER overrides a
    /// hard capability requirement. `Prefer(Grok)` + `requires: vision` where
    /// Grok's qualification flipped vision off must land on the vision-capable
    /// survivor — the preferred target is excluded with the rest, before any
    /// scoring or promotion.
    #[tokio::test]
    async fn hard_gate_excludes_a_preferred_target_before_scoring() {
        let mut grok = realtime_expert();
        grok.caps = Some(ProviderCapabilities {
            browser: true, // qualification flipped Grok's vision off
            ..ProviderCapabilities::default()
        });

        let router = ExpertRouter::new(vec![Box::new(grok), Box::new(deep_expert())]);
        let r = router
            .consult(
                &ReasoningRequest::new("describe this image")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Grok))
                    .requires(&[ProviderCapability::Vision]),
            )
            .await
            .unwrap();
        assert_eq!(
            r.provider, "deep-expert",
            "preference never bypasses the gate"
        );
        assert!(r.capabilities.supports(ProviderCapability::Vision));
    }

    /// Duplicate identities collapse to the first registered: evidence is
    /// keyed by identity, so a second registration of the same identity would
    /// otherwise silently compete against — and merge telemetry into — the
    /// first. First registered wins, deterministically.
    #[tokio::test]
    async fn duplicate_identities_collapse_to_first_registered() {
        // A second ChatGPT whose profile would out-rank the first — it must
        // never be offered a question, even under `Automatic`.
        let shadow = FakeProvider {
            id: "shadow-chatgpt",
            profile: ExpertProfile::new(1.0, 1.0, 1.0, 1.0, 1.0),
            fail: false,
            provider_id: ProviderId::ChatGPT,
            caps: None,
        };
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(shadow)]);
        let r = router
            .consult(&ReasoningRequest::new("debug this rust panic"))
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert", "first registered identity wins");
        // Same under a targeting policy.
        let best = router
            .best(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::ChatGPT)),
            )
            .unwrap()
            .id()
            .to_string();
        assert_eq!(best, "code-expert");
    }

    /// `Fastest` with no measured evidence at all: every provider's fastness is
    /// 0.0 — no speed claims without samples — so the order falls back to
    /// registration order (a stable tie), never an error or an arbitrary pick.
    #[tokio::test]
    async fn fastest_with_no_measured_providers_falls_back_to_registration_order() {
        let router = ExpertRouter::new(vec![
            Box::new(code_expert()),
            Box::new(realtime_expert()),
            Box::new(deep_expert()),
        ]);
        let best = router
            .best(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Fastest))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(
            best, "code-expert",
            "an unmeasured tie falls to the first registered provider"
        );
        let r = router
            .consult(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Fastest))
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert");
    }

    /// `confidence: High` with zero evidence: no provider has any proof, so no
    /// boost applies and the profile fit decides — a preference never
    /// fabricates confidence.
    #[tokio::test]
    async fn confidence_high_with_zero_evidence_falls_back_to_profile() {
        let router = ExpertRouter::new(vec![
            Box::new(realtime_expert()),
            Box::new(code_expert()),
            Box::new(deep_expert()),
        ]);
        let best = router
            .best(
                &ReasoningRequest::new("analyze this deep question")
                    .with_confidence(ConfidenceTier::High),
            )
            .unwrap()
            .id()
            .to_string();
        assert_eq!(best, "deep-expert", "profile decides when nobody is proven");
    }

    /// Extreme (or absurd) latency measurements must never panic or produce a
    /// broken ordering — a u64::MAX sample is clamped everywhere it touches
    /// scoring.
    #[tokio::test]
    async fn extreme_latency_evidence_cannot_break_scoring() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(deep_expert())])
            .with_telemetry(Arc::clone(&telemetry));
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::ChatGPT,
            success: true,
            duration_ms: u64::MAX,
            error: None,
            at_ms: 1,
        });
        telemetry.record(MeasuredOutcome {
            provider: ProviderId::Claude,
            success: true,
            duration_ms: 0,
            error: None,
            at_ms: 1,
        });
        // Under Fastest the measured 0ms expert wins outright; nothing panics.
        let best = router
            .best(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Fastest))
            .expect("extreme evidence must not break best");
        assert_eq!(best.id(), "deep-expert");
        // Even an Automatic + latency:Fast consult works end to end.
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic").with_latency(LatencyTier::Fast),
            )
            .await
            .unwrap();
        assert!(!r.text.is_empty());
    }

    /// Stale evidence still counts today: the frozen contract has no time
    /// decay, so measurements recorded long ago keep shaping the order.
    /// Decay/retention is a future telemetry concern, not router behavior.
    #[tokio::test]
    async fn stale_evidence_still_counts_without_time_decay() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router = ExpertRouter::new(vec![
            Box::new(realtime_expert()), // Grok — measured slow, long ago
            Box::new(code_expert()),     // ChatGPT — measured fast, long ago
            Box::new(deep_expert()),     // Claude — never measured
        ])
        .with_telemetry(Arc::clone(&telemetry));
        for _ in 0..4 {
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::Grok,
                success: true,
                duration_ms: 6000,
                error: None,
                at_ms: 1, // ancient
            });
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::ChatGPT,
                success: true,
                duration_ms: 2000,
                error: None,
                at_ms: 1, // ancient
            });
        }
        let best = router
            .best(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Fastest))
            .unwrap()
            .id()
            .to_string();
        assert_eq!(
            best, "code-expert",
            "ancient measurements still order Fastest (no decay yet)"
        );
    }

    /// A policy beats a preference: `Prefer(Grok)` wins even when Grok is
    /// measured slow and a `latency: Fast` preference is set — the bounded
    /// boost nudges the order; it cannot override a deliberate target.
    #[tokio::test]
    async fn preferred_target_wins_even_when_measured_slow() {
        let telemetry = Arc::new(ProviderTelemetry::new());
        let router = ExpertRouter::new(vec![Box::new(realtime_expert()), Box::new(code_expert())])
            .with_telemetry(Arc::clone(&telemetry));
        for _ in 0..4 {
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::Grok,
                success: true,
                duration_ms: 8000,
                error: None,
                at_ms: 1,
            });
            telemetry.record(MeasuredOutcome {
                provider: ProviderId::ChatGPT,
                success: true,
                duration_ms: 200,
                error: None,
                at_ms: 1,
            });
        }
        let r = router
            .consult(
                &ReasoningRequest::new("debug this rust panic")
                    .with_routing(RoutingPolicy::Prefer(ProviderId::Grok))
                    .with_latency(LatencyTier::Fast),
            )
            .await
            .unwrap();
        assert_eq!(
            r.provider, "realtime-expert",
            "the policy target wins over a bounded latency preference"
        );
    }

    /// A provider advertising a NaN/absurd profile must not panic routing: the
    /// ordering's `partial_cmp` degrades to a stable tie instead.
    #[tokio::test]
    async fn nan_profiles_cannot_panic_routing() {
        let broken = FakeProvider {
            id: "broken",
            profile: ExpertProfile::new(f32::NAN, f32::INFINITY, 0.4, 0.3, 0.1),
            fail: false,
            provider_id: ProviderId::Generic,
            caps: None,
        };
        let router = ExpertRouter::new(vec![Box::new(code_expert()), Box::new(broken)]);
        let best = router
            .best(&ReasoningRequest::new("debug this rust panic"))
            .expect("NaN profiles must not panic best");
        assert_eq!(best.id(), "code-expert");
        let r = router
            .consult(&ReasoningRequest::new("debug this rust panic"))
            .await
            .unwrap();
        assert_eq!(r.provider, "code-expert");
    }

    #[tokio::test]
    async fn best_returns_none_for_an_empty_router() {
        let router = ExpertRouter::new(Vec::new());
        assert!(
            router.best(&ReasoningRequest::new("anything")).is_none(),
            "no providers, no best — never a panic"
        );
    }

    // ── Reserved policies fail loudly ──────────────────────────────────

    #[tokio::test]
    async fn consult_consensus_is_reserved_and_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let err = router
            .consult(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Consensus))
            .await
            .unwrap_err();
        assert!(
            err.contains("routing policy 'consensus' is reserved"),
            "err: {err}"
        );
    }

    #[tokio::test]
    async fn consult_cheapest_is_reserved_and_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let err = router
            .consult(&ReasoningRequest::new("hi").with_routing(RoutingPolicy::Cheapest))
            .await
            .unwrap_err();
        assert!(
            err.contains("routing policy 'cheapest' is reserved"),
            "err: {err}"
        );
    }

    #[tokio::test]
    async fn adapter_consult_reserved_policy_errors() {
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "hi", "routing": "consensus" }),
            ))
            .await;
        assert!(!res.success);
        assert!(
            res.error.unwrap().contains("'consensus' is reserved"),
            "a reserved strategy must error, never silently become automatic"
        );
    }

    // ── ReasoningAdapter (planner-visible) ────────────────────────────

    // `ReasoningAdapter::new()` attaches the persisted telemetry store, which
    // *reads* `~/.friday/telemetry/reasoning-providers.json` if a developer's
    // daemon wrote one. Both tests below are safe because they fail before any
    // consult (so nothing is ever recorded/written) and never assert on stats
    // — keep that invariant if a future test consults through `new()`.

    #[tokio::test]
    async fn adapter_unknown_action_errors() {
        let adapter = ReasoningAdapter::new();
        let res = adapter
            .execute(ActionRequest::new("reasoning", "teleport", json!({})))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("unknown reasoning action"));
    }

    #[tokio::test]
    async fn adapter_consult_requires_a_question() {
        let adapter = ReasoningAdapter::new();
        let res = adapter
            .execute(ActionRequest::new("reasoning", "consult", json!({})))
            .await;
        assert!(!res.success);
        assert!(res.error.unwrap().contains("question"));
    }

    #[tokio::test]
    async fn adapter_consult_routes_through_a_router() {
        // A router whose only provider is a fake API expert — the adapter
        // returns text + provider, proving the planner-path defaults (query
        // param) and the transport-agnostic reply contract.
        let router = ExpertRouter::new(vec![Box::new(code_expert())]);
        let adapter = ReasoningAdapter::with_router(router);
        let res = adapter
            .execute(ActionRequest::new(
                "reasoning",
                "consult",
                json!({ "query": "debug this rust panic" }),
            ))
            .await;
        assert!(res.success, "consult failed: {:?}", res.error);
        assert_eq!(res.output["text"], "answer from code-expert");
        assert_eq!(res.output["provider"], "code-expert");
        assert_eq!(res.output["transport"], "api");
    }

    /// Live browser integration (`FRIDAY_BROWSER_TEST=1`): the browser
    /// transport's `reason` runs the full WebAgent round trip against a local
    /// echo page. Skipped unless the env var is set.
    #[tokio::test]
    async fn browser_provider_reason_roundtrip_local_page() {
        if std::env::var("FRIDAY_BROWSER_TEST")
            .map(|v| v != "1")
            .unwrap_or(true)
        {
            eprintln!(
                "skipping reasoning browser integration (set FRIDAY_BROWSER_TEST=1 to enable)"
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

        // A provider pointing the browser transport at the local echo page.
        let provider = BrowserReasoningProvider::new(
            &base,
            crate::adapters::agent_for_site(&base, crate::adapters::BrowserAdapter::new()),
            ExpertProfile::new(0.5, 0.5, 0.5, 0.4, 0.3),
        );
        assert_eq!(provider.transport(), Transport::Browser);
        let reply = provider.reason("Hello reasoning").await.expect("reason");
        assert!(reply.contains("Hello reasoning"), "reply: {reply}");
        server.abort();
    }

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
