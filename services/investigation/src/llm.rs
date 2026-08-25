//! `LlmInvestigationStrategy` — the model-backed strategy.
//!
//! The LLM derives the information requirements from the goal; the
//! deterministic [`HeuristicInvestigationStrategy`] is the fallback whenever
//! the model call fails, returns unparseable output, or produces no valid
//! requirements. **A failed model is never a failed investigation** — the
//! engine still answers, deterministically, and the fallback is observable
//! via `[Investigation Strategy]` trace lines.
//!
//! The strategy stays model-agnostic: it depends only on
//! [`LlmStrategyClient`], so the Inference Fabric or any OpenAI-compatible
//! client can be wired in without the investigation crate ever depending on
//! a provider.

use async_trait::async_trait;
use serde::Deserialize;

use crate::channel::InvestigationChannel;
use crate::heuristic::HeuristicInvestigationStrategy;
use crate::plan::{InformationRequirement, InvestigationPlan};
use crate::strategy::{InvestigationStrategy, LlmStrategyClient};

/// Model-backed strategy with heuristic fallback.
///
/// Drop-in for [`HeuristicInvestigationStrategy`]: constructed the same way,
/// implements the same trait, consumed by the same engine.
pub struct LlmInvestigationStrategy {
    client: Box<dyn LlmStrategyClient>,
    fallback: HeuristicInvestigationStrategy,
    /// Upper bound on requirements the model may contribute (protects the
    /// planner from a runaway list; strongest requirements survive).
    max_requirements: usize,
}

impl LlmInvestigationStrategy {
    /// Wrap a model client with the default heuristic fallback.
    pub fn new(client: Box<dyn LlmStrategyClient>) -> Self {
        Self {
            client,
            fallback: HeuristicInvestigationStrategy::default(),
            max_requirements: 8,
        }
    }

    /// Swap the fallback (e.g. a stricter heuristic threshold).
    pub fn with_fallback(mut self, fallback: HeuristicInvestigationStrategy) -> Self {
        self.fallback = fallback;
        self
    }

    /// Cap the number of requirements the model may contribute.
    pub fn with_max_requirements(mut self, max: usize) -> Self {
        self.max_requirements = max.max(1);
        self
    }

    /// The prompt handed to the model: strict JSON contract, channel
    /// vocabulary, and the rule that an empty list is a valid answer.
    fn prompt(&self, goal: &str) -> String {
        format!(
            "You are the Investigation Engine of FRIDAY, a cognitive desktop agent.\n\
             Given a goal, decide which information channels must be consulted BEFORE\n\
             executing it. Only include channels the goal genuinely depends on.\n\
             \n\
             Channels:\n\
             - web       : external knowledge (search the web, fetch pages)\n\
             - docs      : project or technical documentation\n\
             - codeintel : understanding the current repository / codebase\n\
             - memory    : recall of prior conversations, user preferences, history\n\
             - git       : repository status, history, branches, working tree\n\
             - filesystem: local files and directories\n\
             - external  : external reasoning through a logged-in website (ask ChatGPT / Grok / \
                           Gemini via the browser — no API keys)\n\
             \n\
             Respond with ONLY a JSON object, no prose:\n\
             {{\"requirements\":[{{\"channel\":\"git\",\"rationale\":\"goal mentions repo status\",\"confidence\":0.9}}]}}\n\
             \n\
             Rules:\n\
             - confidence is 0.0..=1.0 (how strongly the goal needs that channel)\n\
             - rationale is a short phrase from the goal that triggered the channel\n\
             - an empty list [] means the goal needs no investigation before planning\n\
             \n\
             Goal: {goal}\n"
        )
    }
}

/// The result of turning a model response into a plan.
enum ParseOutcome {
    Plan(InvestigationPlan),
    /// Fall back to the heuristic; carries the observability reason.
    Fallback(&'static str),
}

#[async_trait]
impl InvestigationStrategy for LlmInvestigationStrategy {
    async fn investigate(&self, goal: &str) -> InvestigationPlan {
        let response = match self.client.complete(&self.prompt(goal)).await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(
                    "[Investigation Strategy] strategy=llm → heuristic / reason=model_error / goal=\"{goal}\" / error={e}"
                );
                return self.fallback.investigate(goal).await;
            }
        };

        match parse_requirements(
            &response,
            self.max_requirements,
            self.fallback.min_confidence,
        ) {
            ParseOutcome::Plan(plan) => {
                tracing::info!(
                    "[Investigation Strategy] strategy=llm / requirements={} / goal=\"{goal}\"",
                    plan.requirements.len()
                );
                plan
            }
            ParseOutcome::Fallback(reason) => {
                tracing::warn!(
                    "[Investigation Strategy] strategy=llm → heuristic / reason={reason} / goal=\"{goal}\""
                );
                self.fallback.investigate(goal).await
            }
        }
    }
}

/// The lenient wire DTO. `rationale`/`confidence` are optional so a model
/// omitting either is tolerated; channel names are validated separately.
#[derive(Debug, Deserialize)]
struct RequirementDto {
    channel: String,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
}

/// Parse a model response into a plan, validating every requirement.
///
/// Tolerances (a model is a model, not a parser):
/// - code-fence / surrounding-prose wrapping is stripped;
/// - both `{"requirements":[...]}` and a bare `[...]` are accepted;
/// - hallucinated channels are dropped, never fatal;
/// - non-finite confidence values drop the entry; out-of-range values clamp;
/// - missing confidence defaults to 0.5 (neutral);
/// - duplicate channels keep the strongest entry;
/// - entries below the heuristic's `min_confidence` floor are dropped, so
///   "the model said 0.01" never forces an evidence plan;
/// - the list is capped at `max` and sorted by confidence desc (ties on
///   stable channel order, exactly like the heuristic).
///
/// Falls back when the text is unparseable or every entry is invalid. An
/// *explicitly empty* `[]` is honored (the model judged the goal
/// self-contained); a non-empty list whose entries are all garbage falls
/// back — garbage must never read as "no investigation needed".
fn parse_requirements(text: &str, max: usize, min_confidence: f32) -> ParseOutcome {
    let value: serde_json::Value = match serde_json::from_str(extract_json(text)) {
        Ok(v) => v,
        Err(_) => return ParseOutcome::Fallback("unparseable"),
    };

    let raw_entries: Vec<&serde_json::Value> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        let Some(arr) = value.get("requirements").and_then(|v| v.as_array()) else {
            return ParseOutcome::Fallback("unparseable");
        };
        arr.iter().collect()
    };

    // Distinguish "the model said nothing is needed" (raw list empty) from
    // "the model emitted entries, but none of them were usable". Only the
    // first may produce an empty plan; the second falls back to the heuristic.
    if raw_entries.is_empty() {
        return ParseOutcome::Plan(InvestigationPlan::default());
    }

    let mut best: Vec<InformationRequirement> = Vec::new();
    for raw in raw_entries {
        let Ok(entry) = serde_json::from_value::<RequirementDto>(raw.clone()) else {
            continue; // malformed entry (`null`, number, non-string channel) — drop
        };
        let Some(channel) = InvestigationChannel::from_label(&entry.channel) else {
            continue; // hallucinated channel — drop
        };
        let confidence = match entry.confidence {
            Some(c) if c.is_finite() => c.clamp(0.0, 1.0),
            Some(_) => continue, // NaN/Inf — drop the entry
            None => 0.5,
        };
        if confidence < min_confidence {
            continue; // same floor as the heuristic — "0.01" never forces evidence
        }
        let rationale = entry
            .rationale
            .filter(|r| !r.trim().is_empty())
            .unwrap_or_else(|| channel.label().to_string());
        let req = InformationRequirement {
            channel,
            rationale,
            confidence,
        };
        if let Some(existing) = best.iter_mut().find(|r| r.channel == channel) {
            if req.confidence > existing.confidence {
                *existing = req;
            }
        } else {
            best.push(req);
        }
    }

    if best.is_empty() {
        return ParseOutcome::Fallback("no_valid_requirements");
    }

    // Same ordering contract as the heuristic: confidence desc, ties break on
    // stable channel order so identical input always yields identical plans.
    best.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.channel.ordinal().cmp(&b.channel.ordinal()))
    });
    best.truncate(max);

    ParseOutcome::Plan(InvestigationPlan { requirements: best })
}

/// Extract the JSON region from a model response: tolerates ```json fences
/// and leading/trailing prose, keeping the first balanced `{…}`/`[…]` region.
fn extract_json(text: &str) -> &str {
    let t = text.trim();
    if let Some(fenced) = t.strip_prefix("```") {
        let inner = fenced.split("```").next().unwrap_or("");
        return extract_region(inner);
    }
    extract_region(t)
}

fn extract_region(s: &str) -> &str {
    let t = s.trim_start();
    // The outermost structure opens first — so when prose wraps the JSON, the
    // first `{`/`[` (whichever comes first) marks the start, and its matching
    // closer is the last occurrence of the paired bracket.
    match (t.find('{'), t.find('[')) {
        (Some(o), Some(a)) if o <= a => slice_to(t, o, '}'),
        (Some(_), Some(a)) => slice_to(t, a, ']'),
        (Some(o), None) => slice_to(t, o, '}'),
        (None, Some(a)) => slice_to(t, a, ']'),
        (None, None) => t,
    }
}

fn slice_to(s: &str, from: usize, close: char) -> &str {
    if let Some(end) = s[from..].rfind(close) {
        &s[from..from + end + 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted client so every failure mode is testable deterministically.
    struct MockLlmClient {
        response: Result<String, String>,
        last_prompt: std::sync::Mutex<Option<String>>,
    }

    impl MockLlmClient {
        fn ok(json: &str) -> Self {
            Self {
                response: Ok(json.to_string()),
                last_prompt: std::sync::Mutex::new(None),
            }
        }
        fn err(message: &str) -> Self {
            Self {
                response: Err(message.to_string()),
                last_prompt: std::sync::Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl LlmStrategyClient for MockLlmClient {
        async fn complete(&self, prompt: &str) -> Result<String, String> {
            *self.last_prompt.lock().unwrap() = Some(prompt.to_string());
            self.response.clone()
        }
    }

    fn strategy(client: MockLlmClient) -> LlmInvestigationStrategy {
        LlmInvestigationStrategy::new(Box::new(client))
    }

    #[tokio::test]
    async fn model_response_produces_sorted_validated_plan() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[
                {"channel":"git","rationale":"repo status","confidence":0.8},
                {"channel":"web","rationale":"search the web","confidence":0.95}
            ]}"#,
        );
        let plan = strategy(client).investigate("Check the repo").await;
        assert_eq!(
            plan.channels(),
            vec![InvestigationChannel::Web, InvestigationChannel::Git],
            "highest confidence first: {plan:?}"
        );
        assert_eq!(plan.requirements[0].confidence, 0.95);
        assert_eq!(plan.requirements[1].rationale, "repo status");
    }

    #[tokio::test]
    async fn prompt_contains_channel_vocabulary_and_goal() {
        let s =
            LlmInvestigationStrategy::new(Box::new(MockLlmClient::ok(r#"{"requirements":[]}"#)));
        let prompt = s.prompt("Deploy the release build");
        assert!(prompt.contains("web"), "channel vocabulary: {prompt}");
        assert!(prompt.contains("codeintel"), "channel vocabulary: {prompt}");
        assert!(prompt.contains("Deploy the release build"), "goal echoed");
    }

    #[tokio::test]
    async fn model_error_falls_back_to_heuristic() {
        let client = MockLlmClient::err("provider unavailable");
        // The heuristic fires web for this goal — the fallback must produce it.
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(
            plan.channels().contains(&InvestigationChannel::Web),
            "heuristic fallback: {plan:?}"
        );
    }

    #[tokio::test]
    async fn unparseable_response_falls_back_to_heuristic() {
        let client = MockLlmClient::ok("sorry, I cannot help with that");
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Web));
    }

    #[tokio::test]
    async fn object_missing_requirements_falls_back() {
        let client = MockLlmClient::ok(r#"{"channels":["web"]}"#);
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(plan.channels().contains(&InvestigationChannel::Web));
    }

    #[tokio::test]
    async fn explicit_empty_list_is_honored_not_fallback() {
        let client = MockLlmClient::ok(r#"{"requirements":[]}"#);
        // The heuristic WOULD require web for this goal — the model's judgment
        // (empty) must win, proving the fallback did not fire.
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(!plan.needs_investigation(), "{plan:?}");
    }

    #[tokio::test]
    async fn bare_array_is_accepted() {
        let client =
            MockLlmClient::ok(r#"[{"channel":"memory","rationale":"recall","confidence":0.7}]"#);
        let plan = strategy(client)
            .investigate("Continue what we did before")
            .await;
        assert_eq!(plan.channels(), vec![InvestigationChannel::Memory]);
    }

    #[tokio::test]
    async fn prose_wrapped_bare_array_parses() {
        let client = MockLlmClient::ok(
            "The channels are: [{\"channel\":\"git\",\"rationale\":\"git\",\"confidence\":0.6}] as requested.",
        );
        let plan = strategy(client).investigate("Check git").await;
        assert_eq!(plan.channels(), vec![InvestigationChannel::Git]);
    }

    #[tokio::test]
    async fn fenced_and_prose_wrapped_json_parses() {
        let client = MockLlmClient::ok(
            "Here you go:\n```json\n{\"requirements\":[{\"channel\":\"git\",\"rationale\":\"git\",\"confidence\":0.6}]}\n```\nHope that helps!",
        );
        let plan = strategy(client).investigate("Check git").await;
        assert_eq!(plan.channels(), vec![InvestigationChannel::Git]);
    }

    #[tokio::test]
    async fn hallucinated_channels_are_dropped() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[
                {"channel":"teleport","rationale":"beam me up","confidence":0.9},
                {"channel":"git","rationale":"status","confidence":0.7}
            ]}"#,
        );
        let plan = strategy(client).investigate("Check git").await;
        assert_eq!(
            plan.channels(),
            vec![InvestigationChannel::Git],
            "teleport must be dropped: {plan:?}"
        );
    }

    #[tokio::test]
    async fn all_invalid_entries_fall_back_to_heuristic() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[{"channel":"teleport","rationale":"x","confidence":0.9}]}"#,
        );
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(
            plan.channels().contains(&InvestigationChannel::Web),
            "fallback because nothing was valid: {plan:?}"
        );
    }

    #[tokio::test]
    async fn confidence_is_clamped_and_missing_defaults() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[
                {"channel":"web","rationale":"w","confidence":99.0},
                {"channel":"memory","rationale":"m"},
                {"channel":"git","rationale":"g","confidence":-3.0}
            ]}"#,
        );
        let plan = strategy(client).investigate("anything").await;
        let web = plan
            .requirements
            .iter()
            .find(|r| r.channel == InvestigationChannel::Web)
            .unwrap();
        assert_eq!(web.confidence, 1.0, "clamped down");
        let mem = plan
            .requirements
            .iter()
            .find(|r| r.channel == InvestigationChannel::Memory)
            .unwrap();
        assert_eq!(mem.confidence, 0.5, "missing defaults to neutral");
        assert!(
            !plan
                .requirements
                .iter()
                .any(|r| r.channel == InvestigationChannel::Git),
            "clamped-to-0.0 entry is below the min-confidence floor: {plan:?}"
        );
    }

    #[tokio::test]
    async fn sub_floor_confidence_is_dropped() {
        // A 0.1 requirement must not force an evidence plan. (The dropped
        // entry falls back to the heuristic — so use a goal the heuristic
        // also deems self-contained to observe the empty result.)
        let client = MockLlmClient::ok(
            r#"{"requirements":[{"channel":"git","rationale":"weak","confidence":0.1}]}"#,
        );
        let plan = strategy(client)
            .investigate("Synthesize the quarterly numbers")
            .await;
        assert!(
            !plan.needs_investigation(),
            "0.1 must not force an evidence plan: {plan:?}"
        );
    }

    #[tokio::test]
    async fn garbage_entries_fall_back_instead_of_empty_plan() {
        // A non-empty list whose entries are all malformed must NOT read as
        // "the model judged the goal self-contained" — it falls back.
        let client = MockLlmClient::ok(r#"{"requirements":[null, 123, {"channel":42}]}"#);
        let plan = strategy(client)
            .investigate("Search the web for the latest Rust async docs")
            .await;
        assert!(
            plan.channels().contains(&InvestigationChannel::Web),
            "garbage entries → heuristic fallback: {plan:?}"
        );
    }

    #[tokio::test]
    async fn duplicate_channels_keep_the_strongest() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[
                {"channel":"git","rationale":"weak","confidence":0.4},
                {"channel":"git","rationale":"strong","confidence":0.9}
            ]}"#,
        );
        let plan = strategy(client).investigate("Check git").await;
        assert_eq!(plan.requirements.len(), 1);
        assert_eq!(plan.requirements[0].confidence, 0.9);
        assert_eq!(plan.requirements[0].rationale, "strong");
    }

    #[tokio::test]
    async fn max_requirements_caps_the_list() {
        let client = MockLlmClient::ok(
            r#"{"requirements":[
                {"channel":"web","rationale":"1","confidence":0.9},
                {"channel":"git","rationale":"2","confidence":0.8},
                {"channel":"memory","rationale":"3","confidence":0.7}
            ]}"#,
        );
        let s = strategy(client).with_max_requirements(2);
        let plan = s.investigate("anything").await;
        assert_eq!(plan.requirements.len(), 2);
        // The strongest survive.
        assert_eq!(plan.channels()[0], InvestigationChannel::Web);
        assert_eq!(plan.channels()[1], InvestigationChannel::Git);
    }
}
