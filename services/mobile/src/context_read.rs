//! **R5-2-5-2 — Screen context read (daemon integration vertical slice).**
//!
//! This is the smallest end-to-end wiring of the frozen R5-2-4/R5-2-5-1 seams:
//!
//! ```text
//! explicit user request
//!        ↓
//! ContextCollector   (Screen capture — OS capability only)
//!        ↓ CapturedContext
//! ContextGate        (user authorization)
//!        ↓
//! EgressGate         (payload safety)
//!        ↓
//! ProviderSeam       (the REAL provider boundary)
//!        ↓
//! FRIDAY response
//! ```
//!
//! Ownership invariants (frozen R5-2-5-2-0):
//!
//! - The **collector** does not know whether context may leave the machine.
//! - The **daemon** does not know how to capture; it only *orchestrates* by
//!   calling this module's [`ScreenReader`].
//! - The **shell** does not become an orchestration layer (chooser deferred to
//!   R5-2-5-3).
//!
//! The slice is deliberately narrow: **one** explicit request, **Screen** only,
//! **no ambient capture**, **no** clipboard/files/mic/OCR/browser, **no** new IPC.
//! [`EgressGate`] is composed here so the happy path is *allowed when safe* and
//! *blocked when a secret is present*, and — crucially — the provider boundary is
//! a trait ([`ProviderSeam`]) so both invariants are proven **at the provider**
//! (a not-consented read never reaches `send`).

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use friday_context_collector::{CaptureOptions, CapturedContext, ContextCollector};
use friday_intelligence::{
    compose, ContextChannel, ContextComposeError, ContextDecision, ContextGate, EgressGate,
    ProviderTarget, SecretsVault,
};

/// The real reasoning-provider boundary. `send` is the **only** way a captured
/// payload reaches a provider in this slice. A test double recording whether
/// `send` was called is what proves the not-consented invariant at the actual
/// boundary (not a boolean upstream).
#[async_trait]
pub trait ProviderSeam: Send + Sync {
    /// Send a composed, gate-approved payload to the provider. Returns the
    /// provider's text response.
    async fn send(&self, payload: &str) -> Result<String, String>;
}

/// Why an explicit Screen context read did not produce a provider response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextReadError {
    /// The OS capture itself failed (command / timeout / invalid options).
    Capture(String),
    /// The channel was not a Screen capture payload (unreachable for Screen).
    InvalidPayload(String),
    /// ContextGate refused — the user never gave per-use consent. The provider
    /// **never sees the payload** (frozen invariant 2).
    NotAuthorized {
        reason: String,
        disclosure: Option<&'static str>,
    },
    /// Authorized, but EgressGate judged the payload unsafe to leave.
    EgressBlocked { reason: String },
    /// The provider seam refused or errored after receiving an allowed payload.
    Provider(String),
}

impl std::fmt::Display for ContextReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextReadError::Capture(m) => write!(f, "screen capture failed: {m}"),
            ContextReadError::InvalidPayload(m) => write!(f, "invalid screen payload: {m}"),
            ContextReadError::NotAuthorized { reason, .. } => write!(f, "not authorized: {reason}"),
            ContextReadError::EgressBlocked { reason } => write!(f, "egress blocked: {reason}"),
            ContextReadError::Provider(m) => write!(f, "provider error: {m}"),
        }
    }
}

/// Build the text payload that travels the gate chain for a captured Screen.
///
/// Screen yields raw PNG bytes; base64-encoding them lets the real (text/vision)
/// provider seam receive the actual capture rather than an empty stub, while
/// still passing through the string-based [`compose`] chain. Full image/vision
/// delivery is qualified on the Omarchy target in R5-2-5-4.
fn screen_payload(captured: &CapturedContext) -> Result<String, ContextReadError> {
    match captured {
        CapturedContext::Image {
            bytes,
            mime,
            geometry,
        } => {
            let geom = geometry
                .map(|g| g.slurp())
                .unwrap_or_else(|| "fullscreen".to_string());
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(format!(
                "SCREEN_CAPTURE mime={mime} geometry={geom} bytes={} data={encoded}",
                bytes.len()
            ))
        }
        other => Err(ContextReadError::InvalidPayload(format!(
            "expected an image capture for Screen, got {:?}",
            std::mem::discriminant(other)
        ))),
    }
}

/// The explicit Screen context read: collector → ContextGate → EgressGate →
/// provider. `C` is any [`CommandRunner`] (fake on Windows CI / `Real` on the
/// Omarchy target); `P` is the provider seam.
#[derive(Clone)]
pub struct ScreenReader<C, P>
where
    C: friday_context_collector::CommandRunner,
    P: ProviderSeam + Clone,
{
    collector: ContextCollector<C>,
    gate: Arc<std::sync::Mutex<ContextGate>>,
    egress: Arc<EgressGate>,
    provider: P,
    provider_name: String,
    target: ProviderTarget,
}

impl<C, P> ScreenReader<C, P>
where
    C: friday_context_collector::CommandRunner,
    P: ProviderSeam + Clone,
{
    /// Build a Screen reader backed by a fresh (empty) gate and an egress gate
    /// over a fresh vault. No channel is consented until the caller (or the
    /// shell chooser in a later slice) calls [`ScreenReader::consent`].
    pub fn new(collector: ContextCollector<C>, provider: P, provider_name: &str) -> Self {
        let vault = Arc::new(SecretsVault::new());
        Self {
            collector,
            gate: Arc::new(std::sync::Mutex::new(ContextGate::new())),
            egress: Arc::new(EgressGate::new(vault)),
            provider,
            provider_name: provider_name.to_string(),
            target: ProviderTarget::FreeTier,
        }
    }

    /// Record explicit per-use consent for a Screen capture (the shell / user
    /// grants it). Not granted by default — a read without consent is blocked.
    pub fn consent(&self) {
        self.gate.lock().unwrap().consent(ContextChannel::Screen);
    }

    /// Immediate revocation of any pending Screen consent.
    pub fn revoke(&self) {
        self.gate.lock().unwrap().revoke(ContextChannel::Screen);
    }

    /// Is Screen currently consented (inspection for UI/disclosure)?
    pub fn is_consented(&self) -> bool {
        self.gate
            .lock()
            .unwrap()
            .is_consented(ContextChannel::Screen)
    }

    /// Perform one explicit Screen read. Captures, authorizes, egress-checks,
    /// then — **and only then** — hands the payload to the provider.
    pub async fn read(&self) -> Result<String, ContextReadError> {
        self.ask(None).await
    }

    /// Like [`ScreenReader::read`], but the provider answers `question` about
    /// the screen instead of describing it. The question travels inside the
    /// payload, so it goes through the same ContextGate / EgressGate.
    pub async fn ask(&self, question: Option<&str>) -> Result<String, ContextReadError> {
        let captured = self
            .collector
            .capture(
                ContextChannel::Screen,
                &CaptureOptions {
                    region: None,
                    fullscreen: true,
                },
            )
            .map_err(|e| ContextReadError::Capture(e.to_string()))?;

        let mut payload = screen_payload(&captured)?;
        if let Some(q) = question.map(str::trim).filter(|q| !q.is_empty()) {
            // One line: the provider reads everything after `QUESTION: `.
            payload.push_str("\nQUESTION: ");
            payload.push_str(&q.replace(['\n', '\r'], " "));
        }

        let composed = {
            let mut gate = self.gate.lock().unwrap();
            compose(
                &mut gate,
                &self.egress,
                ContextChannel::Screen,
                &self.provider_name,
                self.target,
                &payload,
            )
        };

        let composed = match composed {
            Ok(c) => c,
            Err(ContextComposeError::NotAuthorized { reason, disclosure }) => {
                return Err(ContextReadError::NotAuthorized { reason, disclosure })
            }
            Err(ContextComposeError::EgressBlocked { reason }) => {
                return Err(ContextReadError::EgressBlocked { reason })
            }
        };

        self.provider
            .send(&composed)
            .await
            .map_err(ContextReadError::Provider)
    }

    // --- Re-exposed authorize for library/test drivers that want the exact
    // frozen `ContextGate` decision verbatim. ---
    #[doc(hidden)]
    pub fn authorize_decision(&self) -> ContextDecision {
        self.gate
            .lock()
            .unwrap()
            .authorize(ContextChannel::Screen, self.target)
    }
}

/// A [`ProviderSeam`] backed by the real [`friday_intelligence::InferenceFabric`].
///
/// Routes the gated Screen payload to a **vision** capability model so the
/// actual capture bytes can inform the reply. The model/vendor resolution and
/// live Wayland capture are qualified on the Omarchy target (R5-2-5-4); on a
/// Windows dev host the daemon's tests drive [`FakeCommandRunner`] + a scripted
/// seam instead, so a passing Windows unit test is never mistaken for proof that
/// Linux `grim` capture works.
#[derive(Clone)]
pub struct FabricProviderSeam {
    fabric: Arc<friday_intelligence::InferenceFabric>,
    provider: String,
}

impl FabricProviderSeam {
    pub fn new(fabric: Arc<friday_intelligence::InferenceFabric>, provider: &str) -> Self {
        Self {
            fabric,
            provider: provider.to_string(),
        }
    }
}

#[async_trait]
impl ProviderSeam for FabricProviderSeam {
    async fn send(&self, payload: &str) -> Result<String, String> {
        self.fabric
            .complete_tier(
                &self.provider,
                friday_intelligence::CapabilityTier::VisionFree,
                payload,
                false,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_context_collector::{FakeCommandRunner, ScreenRegion};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nreal-screen-bytes";

    /// A recording provider seam: counts how many payloads reached the boundary
    /// and records the last one. This is the **provider boundary** in the tests —
    /// the invariant assertions inspect `sent()`.
    #[derive(Clone, Default)]
    struct RecordingSeam {
        sends: Arc<AtomicUsize>,
        last: Arc<std::sync::Mutex<Option<String>>>,
        fail: bool,
    }

    #[async_trait]
    impl ProviderSeam for RecordingSeam {
        async fn send(&self, payload: &str) -> Result<String, String> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some(payload.to_string());
            if self.fail {
                Err("upstream refused".to_string())
            } else {
                Ok("FRIDAY: I can see your screen.".to_string())
            }
        }
    }

    impl RecordingSeam {
        fn sent(&self) -> usize {
            self.sends.load(Ordering::SeqCst)
        }
        fn last(&self) -> Option<String> {
            self.last.lock().unwrap().clone()
        }
        fn failing() -> Self {
            RecordingSeam {
                fail: true,
                ..Default::default()
            }
        }
    }

    fn fake_collector() -> ContextCollector<FakeCommandRunner> {
        let runner = FakeCommandRunner::new()
            .script_stdout("grim", &["-"], PNG)
            .script_stdout("grim", &["-g", "0,0 1920x1080", "-"], PNG);
        ContextCollector::new(runner)
    }

    #[tokio::test]
    async fn consented_screen_reach_provider() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        reader.consent();
        assert!(reader.is_consented());
        let resp = reader.read().await.unwrap();
        assert_eq!(resp, "FRIDAY: I can see your screen.");
        // The payload actually reached the provider, carrying the screen bytes.
        let sent = reader.provider.last().unwrap();
        assert!(sent.contains("SCREEN_CAPTURE"));
        assert!(sent.contains("data="));
        assert!(sent.contains(&base64::engine::general_purpose::STANDARD.encode(PNG)));
    }

    #[tokio::test]
    async fn question_travels_with_the_screenshot() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        reader.consent();
        reader
            .ask(Some("what does this\nerror mean?"))
            .await
            .unwrap();
        let sent = reader.provider.last().unwrap();
        let (fields, question) = sent.split_once('\n').expect("question on its own line");
        assert!(fields.starts_with("SCREEN_CAPTURE"));
        assert_eq!(question, "QUESTION: what does this error mean?");
    }

    #[tokio::test]
    async fn asking_still_requires_consent() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        assert!(reader.ask(Some("what is this?")).await.is_err());
        assert_eq!(reader.provider.sent(), 0, "no consent: nothing reaches the provider");
    }

    /// Invariant 2: a consenting read must NEVER reach the provider.
    #[tokio::test]
    async fn unconsented_screen_never_reaches_provider() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        assert!(!reader.is_consented());
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::NotAuthorized { .. }));
        // The provider boundary saw NOTHING.
        assert_eq!(reader.provider.sent(), 0);
        assert!(reader.provider.last().is_none());
    }

    /// Consent is per-use: one read consumes it; a second read is blocked again.
    #[tokio::test]
    async fn consent_is_per_use_consumed_by_read() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        reader.consent();
        assert!(reader.read().await.is_ok());
        // First read consumed consent.
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::NotAuthorized { .. }));
        assert_eq!(
            reader.provider.sent(),
            1,
            "second read must not reach provider"
        );
    }

    /// Revocation is immediate: consent then revoke ⇒ blocked at the gate.
    #[tokio::test]
    async fn revoke_is_immediate() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::default(), "fabric");
        reader.consent();
        reader.revoke();
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::NotAuthorized { .. }));
        assert_eq!(reader.provider.sent(), 0);
    }

    /// Authorized + safe payload flows through. A capture failure surfaces
    /// before any consent decision.
    #[tokio::test]
    async fn capture_failure_is_reported_not_fabricated() {
        // No script → FakeCommandRunner fails loudly → CaptureError::Command.
        let runner = FakeCommandRunner::new();
        let reader = ScreenReader::new(
            ContextCollector::new(runner),
            RecordingSeam::default(),
            "fabric",
        );
        reader.consent();
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::Capture(_)));
        assert_eq!(reader.provider.sent(), 0);
    }

    /// A provider refusal after an allowed payload surfaces as Provider error —
    /// but only after the gates allowed the read through.
    #[tokio::test]
    async fn provider_error_surfaces_after_gates_allow() {
        let reader = ScreenReader::new(fake_collector(), RecordingSeam::failing(), "fabric");
        reader.consent();
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::Provider(_)));
        assert_eq!(
            reader.provider.sent(),
            1,
            "the gated payload did reach the provider"
        );
    }

    /// Region support is delegated to the collector; a fullscreen read uses `grim -`.
    #[test]
    fn fullscreen_read_builds_correct_payload() {
        let payload = screen_payload(&CapturedContext::Image {
            bytes: PNG.to_vec(),
            mime: "image/png",
            geometry: Some(ScreenRegion::new(0, 0, 1920, 1080)),
        })
        .unwrap();
        assert!(payload.contains("geometry=0,0 1920x1080"));
        assert!(payload.contains("mime=image/png"));
    }
}
