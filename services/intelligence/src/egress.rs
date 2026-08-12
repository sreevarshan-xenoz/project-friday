//! Egress policy — the outbound-data gate that answers the autonomy
//! prerequisite questions: *"can this secret leave the machine? which
//! provider is receiving it?"*
//!
//! The constitution's missing piece: *"No 'scan payload before sending to an
//! external model' gate … A payload containing a detected secret is
//! blocked/redacted before the provider call; policy is enforceable
//! per-provider."* The permission engine gates what FRIDAY *does*; this gate
//! gates what FRIDAY *sends*.
//!
//! ```text
//! outbound payload (prompt) + provider name
//!      ↓
//! EgressGate::process
//!      ↓  SecretsVault::scan  (registered values + name/count only)
//!      ↓  per-provider EgressMode
//!      ↓
//!   Allow  ──→ original payload leaves
//!   Redact ──→ [REDACTED:scope/name] payload leaves
//!   Block  ──→ Err — the payload never leaves
//! ```
//!
//! **Safe default:** [`EgressGate::new`] defaults to [`EgressMode::Block`] —
//! an attached gate never lets a detected secret out to an unlisted provider.
//! Redact / LogOnly are explicit per-provider opt-ins. The gate is **off
//! until attached** (`InferenceFabric.egress = Some(…)`), so existing
//! behavior is unchanged by default; enabling it is one line.
//!
//! The verdict's [`SecretHit`]s carry names and counts only — the gate never
//! sees raw values (the vault's scan is the only contact point).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::secrets::{SecretHit, SecretsVault};

/// What the gate does when an outbound payload carries a detected secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressMode {
    /// Refuse to send — the payload never leaves the machine.
    Block,
    /// Replace every detected secret with `[REDACTED:scope/name]` and send
    /// the sanitized payload.
    Redact,
    /// Send, but record the hits on the telemetry/audit path for the policy
    /// owner to review. Never the default.
    LogOnly,
}

/// The gate's decision for one outbound payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EgressAction {
    Allow,
    /// Refused before the call — the reason names the hits (never values).
    Block {
        reason: String,
    },
    Redact,
}

/// One inspection result — safe to log, serialize, and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EgressVerdict {
    pub provider: String,
    pub action: EgressAction,
    /// What was found: names and counts only, never values.
    pub hits: Vec<SecretHit>,
}

/// The outbound-data gate. Owns the vault reference and the per-provider
/// policy table; pure (no I/O), so every decision is deterministic and
/// testable.
pub struct EgressGate {
    vault: Arc<SecretsVault>,
    policies: HashMap<String, EgressMode>,
    default_mode: EgressMode,
}

impl EgressGate {
    /// `default_mode` is [`EgressMode::Block`]: with a gate attached, an
    /// unlisted provider never sends a detected secret out. Opt into
    /// Redact / LogOnly per provider via [`EgressGate::with_provider_policy`].
    pub fn new(vault: Arc<SecretsVault>) -> Self {
        Self {
            vault,
            policies: HashMap::new(),
            default_mode: EgressMode::Block,
        }
    }

    /// Per-provider policy — `provider` names the outbound destination
    /// (`openrouter`, `groq`, …), matching the fabric's provider registry.
    pub fn with_provider_policy(mut self, provider: impl Into<String>, mode: EgressMode) -> Self {
        self.policies.insert(provider.into(), mode);
        self
    }

    /// Override the default for unlisted providers.
    pub fn with_default_mode(mut self, mode: EgressMode) -> Self {
        self.default_mode = mode;
        self
    }

    /// The effective policy for `provider`.
    pub fn mode_for(&self, provider: &str) -> EgressMode {
        self.policies
            .get(provider)
            .copied()
            .unwrap_or(self.default_mode)
    }

    /// Inspect an outbound payload: scan for registered secrets and apply the
    /// provider's policy. Pure — the caller decides what to send.
    pub fn inspect(&self, provider: &str, payload: &str) -> EgressVerdict {
        let hits = self.vault.scan(payload);
        let action = if hits.is_empty() {
            EgressAction::Allow
        } else {
            match self.mode_for(provider) {
                EgressMode::Block => EgressAction::Block {
                    reason: format!(
                        "{} detected secret(s): {}",
                        hits.len(),
                        describe_hits(&hits)
                    ),
                },
                EgressMode::Redact => EgressAction::Redact,
                EgressMode::LogOnly => {
                    tracing::warn!(
                        provider,
                        hits = ?hits,
                        "egress: secrets detected in outbound payload (log-only policy)"
                    );
                    EgressAction::Allow
                }
            }
        };
        EgressVerdict {
            provider: provider.to_string(),
            action,
            hits,
        }
    }

    /// The outbound seam: process a payload about to leave to `provider`.
    /// - `Block` → `Err` with a reason (the payload never leaves).
    /// - `Redact` → `Ok` with every detected secret replaced.
    /// - `Allow` / `LogOnly` → `Ok` unchanged.
    pub fn process(&self, provider: &str, payload: &str) -> Result<String, String> {
        let verdict = self.inspect(provider, payload);
        match verdict.action {
            EgressAction::Allow => Ok(payload.to_string()),
            EgressAction::Block { reason } => Err(format!(
                "egress policy blocked outbound payload to '{provider}': {reason}"
            )),
            EgressAction::Redact => Ok(self.vault.redact(payload)),
        }
    }
}

impl std::fmt::Debug for EgressGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EgressGate")
            .field("vault", &self.vault)
            .field("policies", &self.policies)
            .field("default_mode", &self.default_mode)
            .finish()
    }
}

/// Human-readable hit summary — names and counts only.
fn describe_hits(hits: &[SecretHit]) -> String {
    hits.iter()
        .map(|h| format!("{}×{}", h.name, h.count))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_with_secret(secret_value: &str) -> (Arc<SecretsVault>, EgressGate) {
        let vault = Arc::new(SecretsVault::new());
        vault.store("provider", "openrouter", secret_value);
        let gate = EgressGate::new(Arc::clone(&vault));
        (vault, gate)
    }

    #[test]
    fn clean_payload_always_allows() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let verdict = gate.inspect("openrouter", "summarize the quarterly report");
        assert_eq!(verdict.action, EgressAction::Allow);
        assert!(verdict.hits.is_empty());
        assert_eq!(
            gate.process("openrouter", "summarize the quarterly report")
                .unwrap(),
            "summarize the quarterly report"
        );
    }

    #[test]
    fn block_mode_blocks_a_registered_secret_before_the_call() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let payload = "here is the key sk-token-abcdef please use it";
        let verdict = gate.inspect("openrouter", payload);
        assert!(matches!(verdict.action, EgressAction::Block { .. }));
        assert_eq!(verdict.hits.len(), 1);
        assert_eq!(verdict.hits[0].name, "provider/openrouter");

        let err = gate.process("openrouter", payload).unwrap_err();
        assert!(err.contains("egress policy blocked"), "{err}");
        assert!(err.contains("provider/openrouter"), "{err}");
        assert!(
            !err.contains("sk-token-abcdef"),
            "reason never carries the value: {err}"
        );
    }

    #[test]
    fn redact_mode_redacts_and_still_sends() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let payload = "use sk-token-abcdef and again sk-token-abcdef";
        let verdict = gate.inspect("openrouter", payload);
        assert_eq!(verdict.action, EgressAction::Redact);
        assert_eq!(verdict.hits[0].count, 2);

        let sent = gate.process("openrouter", payload).unwrap();
        assert!(!sent.contains("sk-token-abcdef"), "{sent}");
        assert_eq!(
            sent.matches("[REDACTED:provider/openrouter]").count(),
            2,
            "{sent}"
        );
    }

    #[test]
    fn log_only_policy_allows_but_reports() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::LogOnly);
        let verdict = gate.inspect("openrouter", "mention sk-token-abcdef here");
        assert_eq!(
            verdict.action,
            EgressAction::Allow,
            "payload still goes out"
        );
        assert_eq!(verdict.hits.len(), 1, "but the hits are reported");
    }

    #[test]
    fn policy_is_enforced_per_provider() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        // openrouter: Redact; groq unlisted → default Block.
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let payload = "contains sk-token-abcdef";

        let verdict = gate.inspect("openrouter", payload);
        assert_eq!(verdict.action, EgressAction::Redact);
        let verdict = gate.inspect("groq", payload);
        assert!(matches!(verdict.action, EgressAction::Block { .. }));
    }

    #[test]
    fn default_mode_can_be_softened() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_default_mode(EgressMode::Redact);
        let verdict = gate.inspect("unlisted-provider", "has sk-token-abcdef");
        assert_eq!(verdict.action, EgressAction::Redact);
    }

    #[test]
    fn verdict_never_carries_secret_values() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let verdict = gate.inspect("openrouter", "the secret sk-token-abcdef is here");
        let debug = format!("{verdict:?}");
        assert!(!debug.contains("sk-token-abcdef"), "{debug}");
        let json = serde_json::to_string(&verdict).unwrap();
        assert!(!json.contains("sk-token-abcdef"), "{json}");
    }

    #[test]
    fn gate_debug_shows_policies_but_never_values() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let debug = format!("{gate:?}");
        assert!(debug.contains("openrouter"), "{debug}");
        assert!(!debug.contains("sk-token-abcdef"), "{debug}");
    }
}
