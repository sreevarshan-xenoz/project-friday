//! Provider Qualification Corpus (Phase 1.5).
//!
//! Each [`Scenario`] is a *fact* we extract from a real LLM provider: connect to
//! its gateway, complete a prompt, verify the response (and — in failure-injection
//! scenarios — verify the Inference Fabric reacts correctly). No architecture change,
//! no new provider trait: we *prove* a provider before it is trusted.

use serde_json::json;

/// A single provider intent under test.
#[derive(Debug, Clone)]
pub struct Intent {
    pub intent: &'static str,
    pub params: serde_json::Value,
}

/// What the harness checks after the action to decide if the fact holds.
#[derive(Debug, Clone)]
pub enum Verify {
    /// Gateway responds to a health probe (reachable).
    Reachable,
    /// A trivial prompt returns a non-empty completion.
    Completion,
    /// A strict-JSON prompt returns parseable, schema-valid JSON.
    Json,
    /// A long-context prompt returns a complete, untruncated structured answer.
    LongContext,
    /// The Inference Fabric reacts to an injected failure (circuit + metrics).
    FailureHandled,
    /// The provider streams completions incrementally (V2 `stream`), and the
    /// harness reassembles tokens without loss.
    Streaming,
    /// The provider exposes a live model catalog (V2 `list_models`), and the
    /// harness verifies it is non-empty and well-shaped.
    DynamicDiscovery,
}

/// Failure injection applied at the harness level (mock providers only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Injection {
    /// Request times out.
    Timeout,
    /// Provider returns HTTP 429 (rate limited).
    Http429,
    /// Provider returns HTTP 500.
    Http500,
    /// Provider returns a malformed / unparseable body.
    MalformedJson,
    /// Connection dropped mid-request.
    Disconnect,
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: &'static str,
    pub group: &'static str,
    /// The action under test.
    pub action: Intent,
    pub verify: Verify,
    pub injection: Option<Injection>,
    /// Failure-injection scenarios can only run in `--mock` mode (we cannot inject
    /// faults into a live gateway), so they are gated out of real-mode runs.
    pub requires_mock: bool,
    /// V2 capability scenarios (Streaming, Dynamic Discovery) need a V2 provider
    /// surface. Gated out of real-mode runs for providers that lack a V2 impl.
    pub requires_v2: bool,
}

fn connectivity() -> Intent {
    Intent {
        intent: "provider.connectivity",
        params: json!({}),
    }
}

fn completion(prompt: &str) -> Intent {
    Intent {
        intent: "provider.completion",
        params: json!({ "prompt": prompt }),
    }
}

/// A prompt that demands a strict, schema-shaped JSON object.
pub fn strict_json_prompt() -> String {
    "Respond with ONLY a valid JSON object and nothing else, no prose, no markdown fences. \
     The object must have exactly these keys: \
     \"name\" (a string), \"age\" (an integer), \"tags\" (an array of strings). \
     Example shape: {\"name\":\"friday\",\"age\":1,\"tags\":[\"ai\",\"agent\"]}"
        .to_string()
}

/// A long-context prompt: a large filler block followed by a precise structured task
/// whose correctness proves the model handled the full context without truncation.
pub fn long_context_prompt() -> String {
    let filler = "The quick brown fox jumps over the lazy dog. ";
    let filler = filler.repeat(200);
    format!(
        "{filler}\n\
         Ignore the irrelevant text above. Output a JSON array of exactly 50 consecutive \
         integers starting at 1 and ending at 50, formatted as [1,2,3,...,50]. \
         Output ONLY that array, no prose, no markdown fences."
    )
}

/// The 001 corpus. Grounded in the provider behaviors FRIDAY depends on:
/// connectivity, completion, JSON, long context, and Fabric-level failure handling.
pub fn all() -> Vec<Scenario> {
    vec![
        Scenario {
            id: "connectivity.reach",
            group: "connectivity",
            action: connectivity(),
            verify: Verify::Reachable,
            injection: None,
            requires_mock: false,
            requires_v2: false,
        },
        Scenario {
            id: "completion.hello",
            group: "completion",
            action: completion("Hello, say the single word 'pong' and nothing else."),
            verify: Verify::Completion,
            injection: None,
            requires_mock: false,
            requires_v2: false,
        },
        Scenario {
            id: "json.strict",
            group: "json",
            action: completion(&strict_json_prompt()),
            verify: Verify::Json,
            injection: None,
            requires_mock: false,
            requires_v2: false,
        },
        Scenario {
            id: "long_context.no_truncation",
            group: "long_context",
            action: completion(&long_context_prompt()),
            verify: Verify::LongContext,
            injection: None,
            requires_mock: false,
            requires_v2: false,
        },
        Scenario {
            id: "failure.timeout",
            group: "failure-injection",
            action: completion("This request should time out."),
            verify: Verify::FailureHandled,
            injection: Some(Injection::Timeout),
            requires_mock: true,
            requires_v2: false,
        },
        Scenario {
            id: "failure.http_429",
            group: "failure-injection",
            action: completion("This request should be rate limited."),
            verify: Verify::FailureHandled,
            injection: Some(Injection::Http429),
            requires_mock: true,
            requires_v2: false,
        },
        Scenario {
            id: "failure.http_500",
            group: "failure-injection",
            action: completion("This request should error."),
            verify: Verify::FailureHandled,
            injection: Some(Injection::Http500),
            requires_mock: true,
            requires_v2: false,
        },
        Scenario {
            id: "failure.malformed_json",
            group: "failure-injection",
            action: completion("This request should return a broken body."),
            verify: Verify::FailureHandled,
            injection: Some(Injection::MalformedJson),
            requires_mock: true,
            requires_v2: false,
        },
        Scenario {
            id: "failure.disconnect",
            group: "failure-injection",
            action: completion("This request should drop the connection."),
            verify: Verify::FailureHandled,
            injection: Some(Injection::Disconnect),
            requires_mock: true,
            requires_v2: false,
        },
        Scenario {
            id: "streaming.token_stream",
            group: "streaming",
            action: completion("Stream the sentence: 'the agent reasons, then acts'."),
            verify: Verify::Streaming,
            injection: None,
            requires_mock: false,
            requires_v2: true,
        },
        Scenario {
            id: "dynamic_discovery.catalog",
            group: "dynamic_discovery",
            action: Intent {
                intent: "provider.discover",
                params: json!({}),
            },
            verify: Verify::DynamicDiscovery,
            injection: None,
            requires_mock: false,
            requires_v2: true,
        },
    ]
}
