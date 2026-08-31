//! **R5-2-4 — Context Gate**: the single consent boundary between OS context and
//! any provider (frozen spec `R5-2-4-CONTEXT-GATE-SPEC.md`).
//!
//! This gate answers one question, **before** any context channel may travel to a
//! provider:
//!
//! > *Did the user authorize FRIDAY to send this kind of information?*
//!
//! It is deliberately **independent** of [`crate::egress::EgressGate`], which
//! answers the separate question:
//!
//! > *Even if authorized, is this particular payload safe to send?*
//!
//! Two different security questions at the same outbound boundary. This module
//! establishes the **security contract only** — it performs **no** payload
//! transformation (`authorize` never rewrites the payload), **no** browser/CDP
//! automation, and **no** OS UI.
//!
//! Frozen invariants (each pinned by tests):
//!
//! 1. **Provider choice cannot increase the context FRIDAY is allowed to disclose.**
//! 2. **Remote reasoning cannot obtain a context channel merely because that
//!    provider was selected** (routing never implies consent).
//! 3. **Local never consents; remote never bypasses.** A `Local` (privacy=Required)
//!    request bypasses remote consent; a remote provider can never receive an
//!    unauthorized channel regardless of routing.

use std::collections::HashSet;

/// The discrete OS context surfaces the gate can consent to. Each is an
/// **independent** channel: consenting to one never implies consent to another
/// (R5-2-1 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextChannel {
    /// The user's screen / active display region.
    Screen,
    /// The user's current text selection.
    Selection,
    /// The currently focused / active application.
    App,
    /// Files the user has involved in the current reasoning context.
    Files,
    /// Microphone input.
    Mic,
    /// The OS clipboard.
    Clipboard,
}

impl ContextChannel {
    /// Human-readable label for disclosure UI.
    pub fn label(self) -> &'static str {
        match self {
            ContextChannel::Screen => "screen",
            ContextChannel::Selection => "selection",
            ContextChannel::App => "active app",
            ContextChannel::Files => "files",
            ContextChannel::Mic => "microphone",
            ContextChannel::Clipboard => "clipboard",
        }
    }

    /// The remote free-tier data duty (R5-2-2): free-tier reasoning "improves
    /// our products". Disclosed **before** transmission so the user can consent
    /// with full knowledge.
    pub fn free_tier_disclosure(self) -> Option<&'static str> {
        Some(
            "This context channel is sent to a remote provider. On the free tier, this data may be used to improve our products.",
        )
    }
}

/// The gate's answer for one channel, for one use.
///
/// A `Block` carries a reason. Disclosure (when present) must be shown to the
/// user **before** the payload is transmitted (decision 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextDecision {
    /// The channel is authorized for this use.
    Allow,
    /// The channel is not authorized; the reason can drive UX and diagnostics.
    Block { reason: String },
}

impl ContextDecision {
    /// Whether this channel may proceed to [`crate::egress::EgressGate`].
    pub fn allowed(&self) -> bool {
        matches!(self, ContextDecision::Allow)
    }
}

/// The single consent boundary (R5-2-4).
///
/// Owns the **per-use, per-channel** explicit consent state for the current
/// context request. Consent is attributable and revocable; it is **not**
/// cryptographic signing (decision 9 — the survey never established a trust
/// boundary that requires signatures).
#[derive(Debug)]
pub struct ContextGate {
    /// Channels the user has explicitly consented to for the current request.
    consented: HashSet<ContextChannel>,
}

impl Default for ContextGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextGate {
    /// A gate with **no** consented channels. Like [`crate::egress::EgressGate`],
    /// the gate is inert until explicitly attached and consented.
    pub fn new() -> Self {
        ContextGate {
            consented: HashSet::new(),
        }
    }

    /// Record **explicit, attributable, per-use** consent for `channel`.
    ///
    /// This models the user authorizing FRIDAY to disclose *this kind* of
    /// information for the current request only. It does **not** blanket-approve
    /// a channel for a session or a provider (decision 4).
    pub fn consent(&mut self, channel: ContextChannel) {
        self.consented.insert(channel);
    }

    /// **Immediate** revocation of any pending per-use consent for `channel`
    /// (decision 5). A revoked channel can no longer be authorized; it is also
    /// re-checked by the composition seam before transmission.
    pub fn revoke(&mut self, channel: ContextChannel) {
        self.consented.remove(&channel);
    }

    /// Whether `channel` currently holds pending per-use consent (inspection
    /// without consuming). Useful for the UI/disclosure step.
    pub fn is_consented(&self, channel: ContextChannel) -> bool {
        self.consented.contains(&channel)
    }

    pub fn consented_channels(&self) -> impl Iterator<Item = ContextChannel> + '_ {
        self.consented.iter().copied()
    }

    /// Decide whether `channel` may proceed for `target` — the **only** entry
    /// point that honors both the local-bypass and remote-enforcement invariants.
    ///
    /// - `target.local` is `true` for a privacy=Required Local request: it
    ///   **bypasses** remote consent (local never consents) and returns `Allow`.
    /// - `target.local` is `false` (Free / Subscription / Paid remote): the
    ///   channel **must** hold explicit per-use consent. If it does, the consent
    ///   is **consumed** (this is a per-use grant) and `Allow` is returned. If it
    ///   does not, `Block` — and the channel never reaches `EgressGate` as
    ///   provider-bound data.
    ///
    /// The returned disclosure (free-tier duty) is non-`None` for remote targets
    /// and must be shown **before** transmission (decision 6, 8).
    pub fn authorize(
        &mut self,
        channel: ContextChannel,
        target: ProviderTarget,
    ) -> ContextDecision {
        if target.is_local() {
            // Local bypass (frozen invariant 3 / decision 7): no consent needed.
            return ContextDecision::Allow;
        }
        if self.consented.remove(&channel) {
            ContextDecision::Allow
        } else {
            ContextDecision::Block {
                reason: format!(
                    "context gate: no explicit consent for channel '{}' before '{:?}' provider",
                    channel.label(),
                    target
                ),
            }
        }
    }

    /// Disclosure for a channel toward a remote target — must be surfaced to the
    /// user **before** transmission. `None` for local targets (nothing leaves).
    pub fn disclosure_for(
        &self,
        channel: ContextChannel,
        target: ProviderTarget,
    ) -> Option<&'static str> {
        if target.is_local() {
            None
        } else {
            channel.free_tier_disclosure()
        }
    }
}

/// The class of provider a context request would be sent to. `Local` is the
/// privacy=Required class; every other class (`FreeTier`, `Subscription`,
/// `PaidApi`) is remote and requires explicit consent (decisions 6, 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    Local,
    FreeTier,
    Subscription,
    PaidApi,
}

impl ProviderTarget {
    /// Whether this target requires remote consent (everything local-free).
    pub fn requires_remote_consent(self) -> bool {
        !matches!(self, ProviderTarget::Local)
    }

    pub fn is_local(self) -> bool {
        matches!(self, ProviderTarget::Local)
    }
}

/// Compose the two gates at the outbound boundary (frozen §2 / §4).
///
/// Enforces the ordering:
///
/// ```text
/// ContextGate → (authorize)  → EgressGate → (safe?) → provider-bound payload
/// ```
///
/// - If the channel is **not** authorized it returns `Err` **before** the payload
///   ever reaches the [`crate::egress::EgressGate`] — an unauthorized category
///   never becomes provider-bound data.
/// - If authorized, the existing egress gate still independently decides whether
///   the resulting payload is **safe** to leave the machine.
///
/// Returns `Ok(payload)` valid for transmission, or `Err(reason)` plus the
/// disclosure that the caller must have shown before calling.
pub fn compose(
    gate: &mut ContextGate,
    egress: &crate::egress::EgressGate,
    channel: ContextChannel,
    provider: &str,
    target: ProviderTarget,
    payload: &str,
) -> Result<String, ContextComposeError> {
    let decision = gate.authorize(channel, target);
    if !decision.allowed() {
        let ContextDecision::Block { reason } = decision else {
            unreachable!()
        };
        return Err(ContextComposeError::NotAuthorized {
            reason,
            disclosure: gate.disclosure_for(channel, target),
        });
    }

    egress
        .process(provider, payload)
        .map_err(|egress_reason| ContextComposeError::EgressBlocked {
            reason: egress_reason,
        })
}

/// Why a composed transmission was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextComposeError {
    /// The context channel was never authorized — it never reached the egress gate.
    NotAuthorized {
        reason: String,
        disclosure: Option<&'static str>,
    },
    /// Authorized, but the egress gate judged the resulting payload unsafe.
    EgressBlocked { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egress::EgressGate;

    fn secret_egress(payload_contains_secret: bool) -> EgressGate {
        let vault = std::sync::Arc::new(crate::secrets::SecretsVault::new());
        if payload_contains_secret {
            vault.store("provider", "openrouter", "sk-token-abcdef");
        }
        EgressGate::new(vault)
    }

    // ---- Invariant 1: provider choice cannot increase legal disclosure ----
    // Selecting a remote provider grants no channel that the user has not
    // already consented to per-use.
    #[test]
    fn remote_provider_selection_gives_no_unconsented_channel() {
        // No consent at all.
        let mut gate = ContextGate::new();
        let decision = gate.authorize(ContextChannel::Screen, ProviderTarget::FreeTier);
        assert!(
            !decision.allowed(),
            "an unconsented channel must be blocked"
        );
        // Switching providers changes nothing: still no consent.
        let decision = gate.authorize(ContextChannel::Screen, ProviderTarget::PaidApi);
        assert!(
            !decision.allowed(),
            "a richer provider cannot grant disclosure"
        );
    }

    #[test]
    fn channel_consent_is_per_use_not_per_provider() {
        let mut gate = ContextGate::new();
        gate.consent(ContextChannel::Screen);
        // Consumed by the first remote request.
        assert!(gate
            .authorize(ContextChannel::Screen, ProviderTarget::FreeTier)
            .allowed());
        // A second request for the same channel is not automatically authorized.
        assert!(!gate
            .authorize(ContextChannel::Screen, ProviderTarget::FreeTier)
            .allowed());
        // Routing to a different provider cannot resurrect it.
        assert!(!gate
            .authorize(ContextChannel::Screen, ProviderTarget::PaidApi)
            .allowed());
    }

    // ---- Invariant 2: remote reasoning cannot obtain a channel merely because
    // that provider was selected (routing never implies consent) ----
    #[test]
    fn routing_never_implies_consent() {
        let mut gate = ContextGate::new();
        for provider in [
            ProviderTarget::FreeTier,
            ProviderTarget::Subscription,
            ProviderTarget::PaidApi,
        ] {
            for channel in [
                ContextChannel::Screen,
                ContextChannel::Selection,
                ContextChannel::App,
                ContextChannel::Files,
                ContextChannel::Mic,
                ContextChannel::Clipboard,
            ] {
                assert!(
                    !gate.authorize(channel, provider).allowed(),
                    "routing to {:?} must not grant {:?}",
                    provider,
                    channel
                );
            }
        }
    }

    // ---- Invariant 3: local never consents; remote never bypasses ----
    #[test]
    fn local_bypasses_remote_consent_without_consuming_or_recording() {
        let mut gate = ContextGate::new();
        // A local request never needs consent and never consents.
        assert!(gate
            .authorize(ContextChannel::Mic, ProviderTarget::Local)
            .allowed());
        assert!(
            !gate.is_consented(ContextChannel::Mic),
            "local must not record consent"
        );
        // The same channel is still unavailable to a remote provider.
        assert!(!gate
            .authorize(ContextChannel::Mic, ProviderTarget::Subscription)
            .allowed());
    }

    #[test]
    fn local_and_remote_are_orthogonal() {
        let mut gate = ContextGate::new();
        // Local never grants anything to remote.
        assert!(gate
            .authorize(ContextChannel::Clipboard, ProviderTarget::Local)
            .allowed());
        assert!(!gate
            .authorize(ContextChannel::Clipboard, ProviderTarget::FreeTier)
            .allowed());
    }

    // ---- Immediate revocation (decision 5) ----
    #[test]
    fn revocation_is_immediate() {
        let mut gate = ContextGate::new();
        gate.consent(ContextChannel::App);
        assert!(gate.is_consented(ContextChannel::App));
        gate.revoke(ContextChannel::App);
        assert!(!gate.is_consented(ContextChannel::App));
        assert!(!gate
            .authorize(ContextChannel::App, ProviderTarget::Subscription)
            .allowed());
        // Revoking one channel leaves others untouched.
        gate.consent(ContextChannel::Files);
        assert!(gate
            .authorize(ContextChannel::Files, ProviderTarget::Subscription)
            .allowed());
    }

    // ---- Per-channel independence (decision 3) ----
    #[test]
    fn consent_is_per_channel() {
        let mut gate = ContextGate::new();
        gate.consent(ContextChannel::Screen);
        assert!(gate
            .authorize(ContextChannel::Screen, ProviderTarget::PaidApi)
            .allowed());
        assert!(!gate
            .authorize(ContextChannel::Selection, ProviderTarget::PaidApi)
            .allowed());
        assert!(!gate
            .authorize(ContextChannel::Clipboard, ProviderTarget::PaidApi)
            .allowed());
    }

    // ---- Disclosure visible before transmission (decisions 6, 8) ----
    #[test]
    fn remote_carries_disclosure_local_does_not() {
        let gate = ContextGate::new();
        assert!(gate
            .disclosure_for(ContextChannel::Screen, ProviderTarget::Local)
            .is_none());
        for target in [
            ProviderTarget::FreeTier,
            ProviderTarget::Subscription,
            ProviderTarget::PaidApi,
        ] {
            let d = gate.disclosure_for(ContextChannel::Screen, target);
            assert!(
                d.is_some(),
                "remote must surface disclosure before transmission"
            );
            assert!(d.unwrap().to_lowercase().contains("remote"));
        }
    }

    // ---- Composition: ContextGate precedes EgressGate; unauthorized channel
    // never reaches the egress gate as provider-bound data ----
    #[test]
    fn compose_blocks_unauthorized_before_egress() {
        let egress = secret_egress(false);
        let mut gate = ContextGate::new();
        // No consent: blocked at the ContextGate, never reaching EgressGate.
        let err = compose(
            &mut gate,
            &egress,
            ContextChannel::Files,
            "openrouter",
            ProviderTarget::FreeTier,
            "plain file contents with no secret",
        )
        .unwrap_err();
        assert!(matches!(err, ContextComposeError::NotAuthorized { .. }));
    }

    #[test]
    fn compose_authorizes_then_egress_independently_blocks_secret() {
        let egress = secret_egress(true);
        let mut gate = ContextGate::new();
        gate.consent(ContextChannel::Files);
        // Authorized by ContextGate, but egress still blocks the secret within.
        let err = compose(
            &mut gate,
            &egress,
            ContextChannel::Files,
            "openrouter",
            ProviderTarget::FreeTier,
            "here is the key sk-token-abcdef please use it",
        )
        .unwrap_err();
        assert!(matches!(err, ContextComposeError::EgressBlocked { .. }));
        // The per-use consent was consumed: a second authorized channel is needed.
        assert!(!gate.is_consented(ContextChannel::Files));
    }

    #[test]
    fn compose_allows_authorized_and_safe_payload() {
        let egress = secret_egress(false);
        let mut gate = ContextGate::new();
        gate.consent(ContextChannel::Files);
        let out = compose(
            &mut gate,
            &egress,
            ContextChannel::Files,
            "openrouter",
            ProviderTarget::FreeTier,
            "summarize the quarterly report",
        )
        .unwrap();
        assert_eq!(out, "summarize the quarterly report");
        // Per-use: consumed after this transmission.
        assert!(!gate.is_consented(ContextChannel::Files));
    }

    #[test]
    fn compose_local_bypasses_consent_and_still_runs_egress() {
        let egress = secret_egress(false);
        let mut gate = ContextGate::new();
        // Local needs no consent; transmission proceeds through egress.
        let out = compose(
            &mut gate,
            &egress,
            ContextChannel::Mic,
            "local",
            ProviderTarget::Local,
            "transcribe what I said",
        )
        .unwrap();
        assert_eq!(out, "transcribe what I said");
        assert!(
            !gate.is_consented(ContextChannel::Mic),
            "local never consents"
        );
    }
}
