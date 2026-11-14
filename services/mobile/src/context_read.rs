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
    ContextChannel, ContextDecision, ContextGate, EgressGate, ProviderTarget, SecretsVault,
};
use tokio::sync::broadcast::Sender as BroadcastSender;

use crate::protocol::ServerMessage;
use crate::shell_state::{self, ContextReadRecord, ShellState};
use crate::transport::Command;

/// The real reasoning-provider boundary. `send` is the **only** way a captured
/// payload reaches a provider in this slice. A test double recording whether
/// `send` was called is what proves the not-consented invariant at the actual
/// boundary (not a boolean upstream).
#[async_trait]
pub trait ProviderSeam: Send + Sync {
    /// Send a composed, gate-approved payload to the provider. Returns the
    /// provider's text response.
    async fn send(&self, payload: &str) -> Result<String, String>;

    /// Like [`ProviderSeam::send`], reporting the answer so far as it
    /// streams. A seam that can't stream reports the whole answer once.
    async fn send_streaming(
        &self,
        payload: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        let text = self.send(payload).await?;
        progress(&text);
        Ok(text)
    }
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
        self.ask_streaming(question, &|_: &str| {}).await
    }

    /// [`ScreenReader::ask`], with the answer reported as it streams.
    pub async fn ask_streaming(
        &self,
        question: Option<&str>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, ContextReadError> {
        // Check authorization BEFORE capturing the screen — with no consent,
        // nothing is captured at all.
        {
            let mut gate = self.gate.lock().unwrap();
            let decision = gate.authorize(ContextChannel::Screen, self.target);
            if !decision.allowed() {
                let ContextDecision::Block { reason } = decision else {
                    unreachable!()
                };
                let disclosure = gate.disclosure_for(ContextChannel::Screen, self.target);
                return Err(ContextReadError::NotAuthorized { reason, disclosure });
            }
        }

        let captured = match self.collector.capture(
            ContextChannel::Screen,
            &CaptureOptions {
                region: None,
                fullscreen: true,
                // Just the window being asked about; the whole screen
                // when there is none.
                focused_window: true,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                self.revoke();
                return Err(ContextReadError::Capture(e.to_string()));
            }
        };

        let mut payload = match screen_payload(&captured) {
            Ok(p) => p,
            Err(e) => {
                self.revoke();
                return Err(e);
            }
        };
        if let Some(q) = question.map(str::trim).filter(|q| !q.is_empty()) {
            // One line: the provider reads everything after `QUESTION: `.
            payload.push_str("\nQUESTION: ");
            payload.push_str(&q.replace(['\n', '\r'], " "));
        }

        let composed = self
            .egress
            .process(&self.provider_name, &payload)
            .map_err(|reason| ContextReadError::EgressBlocked { reason })?;

        self.provider
            .send_streaming(&composed, progress)
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

/// Manages asynchronous Screen context ask tasks, ensuring:
/// - At most one screen ask runs at a time (concurrent asks get an immediate error)
/// - The daemon's command loop remains unblocked to process other commands
/// - In-flight screen asks can be cancelled via `task.cancel`
#[derive(Clone)]
pub struct ScreenAskManager<C, P>
where
    C: friday_context_collector::CommandRunner + 'static,
    P: ProviderSeam + Clone + 'static,
{
    reader: Arc<ScreenReader<C, P>>,
    active: Arc<tokio::sync::Mutex<Option<ActiveScreenAsk>>>,
}

struct ActiveScreenAsk {
    id: String,
    abort_handle: tokio::task::AbortHandle,
}

/// Drop guard for the spawned screen-ask task.
///
/// If the spawned task panics before normal completion, this guard ensures
/// that `active` is cleared and any pending screen consent is revoked.
struct ScreenAskTaskGuard<C, P>
where
    C: friday_context_collector::CommandRunner + 'static,
    P: ProviderSeam + Clone + 'static,
{
    active: Arc<tokio::sync::Mutex<Option<ActiveScreenAsk>>>,
    task_cmd_id: String,
    reader: Arc<ScreenReader<C, P>>,
    disarmed: bool,
}

impl<C, P> ScreenAskTaskGuard<C, P>
where
    C: friday_context_collector::CommandRunner + 'static,
    P: ProviderSeam + Clone + 'static,
{
    fn new(
        active: Arc<tokio::sync::Mutex<Option<ActiveScreenAsk>>>,
        task_cmd_id: String,
        reader: Arc<ScreenReader<C, P>>,
    ) -> Self {
        Self {
            active,
            task_cmd_id,
            reader,
            disarmed: false,
        }
    }

    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl<C, P> Drop for ScreenAskTaskGuard<C, P>
where
    C: friday_context_collector::CommandRunner + 'static,
    P: ProviderSeam + Clone + 'static,
{
    fn drop(&mut self) {
        if !self.disarmed {
            self.reader.revoke();
            if let Ok(mut guard) = self.active.try_lock() {
                if guard.as_ref().is_some_and(|a| a.id == self.task_cmd_id) {
                    *guard = None;
                }
            } else {
                let active = self.active.clone();
                let task_cmd_id = self.task_cmd_id.clone();
                tokio::spawn(async move {
                    let mut guard = active.lock().await;
                    if guard.as_ref().is_some_and(|a| a.id == task_cmd_id) {
                        *guard = None;
                    }
                });
            }
        }
    }
}

impl<C, P> ScreenAskManager<C, P>
where
    C: friday_context_collector::CommandRunner + 'static,
    P: ProviderSeam + Clone + 'static,
{
    pub fn new(reader: Arc<ScreenReader<C, P>>) -> Self {
        Self {
            reader,
            active: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn reader(&self) -> &Arc<ScreenReader<C, P>> {
        &self.reader
    }

    pub async fn is_in_flight(&self) -> bool {
        self.active.lock().await.is_some()
    }

    /// Try to start a screen ask. If another screen ask is already in flight,
    /// returns an error CommandResult immediately without granting consent or queueing.
    /// Otherwise, runs the ask in its own tokio task.
    pub async fn try_start(
        &self,
        cmd: &Command,
        result_out: BroadcastSender<ServerMessage>,
        state_lock: Arc<tokio::sync::Mutex<ShellState>>,
        state_path: std::path::PathBuf,
    ) -> Result<(), ServerMessage> {
        let mut active_guard = self.active.lock().await;
        if active_guard.is_some() {
            return Err(ServerMessage::CommandResult {
                id: cmd.id.clone(),
                ok: false,
                result: serde_json::json!({ "error": "FRIDAY is already looking at your screen" }),
            });
        }

        let want_consent = cmd
            .params
            .get("consent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if want_consent {
            self.reader.consent();
        }

        let question = cmd
            .params
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let cmd_id = cmd.id.clone();
        let cmd_method = cmd.method.clone();

        let reader = self.reader.clone();
        let active = self.active.clone();
        let task_cmd_id = cmd_id.clone();

        let handle = tokio::spawn(async move {
            let mut panic_guard =
                ScreenAskTaskGuard::new(active.clone(), task_cmd_id.clone(), reader.clone());

            let progress_out = result_out.clone();
            let progress_id = task_cmd_id.clone();
            let last_sent = std::sync::Mutex::new(None::<std::time::Instant>);
            let progress = move |partial: &str| {
                let mut last = last_sent.lock().unwrap();
                if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(150)) {
                    return;
                }
                *last = Some(std::time::Instant::now());
                let _ = progress_out.send(ServerMessage::Event {
                    topic: "context.progress".to_string(),
                    payload: serde_json::json!({ "id": progress_id, "partial": partial }),
                });
            };

            let asked = reader.ask_streaming(question.as_deref(), &progress).await;

            let (ok, result) = match asked {
                Ok(response) => {
                    tracing::info!("[context.read] screen delivered to provider");
                    (true, serde_json::json!({ "response": response }))
                }
                Err(e) => {
                    tracing::warn!("[context.read] {e}");
                    let plain = crate::failures::plain_screen_error(&e);
                    (false, serde_json::json!({ "error": plain }))
                }
            };

            let mut st = state_lock.lock().await;
            st.context_read = Some(ContextReadRecord {
                method: cmd_method,
                ms: shell_state::now_ms(),
                ok,
                channel: "screen".to_string(),
            });
            shell_state::write_state(&state_path, &shell_state::render(&st));
            drop(st);

            // Publish the terminal result message FIRST.
            let _ = result_out.send(ServerMessage::CommandResult {
                id: task_cmd_id.clone(),
                ok,
                result,
            });

            // Clear active only AFTER the terminal result is published,
            // so the upgrade watcher cannot see idle and exit between them.
            let mut guard = active.lock().await;
            if guard.as_ref().is_some_and(|a| a.id == task_cmd_id) {
                *guard = None;
            }
            drop(guard);
            panic_guard.disarm();
        });

        *active_guard = Some(ActiveScreenAsk {
            id: cmd_id,
            abort_handle: handle.abort_handle(),
        });

        Ok(())
    }

    /// Cancel an in-flight screen ask if one is running and matches `task_id`.
    /// Matches if `task_id` is empty, `"screen"`, or matches the in-flight command id.
    /// Aborts the background task, revokes consent, and publishes the cancelled shape.
    pub async fn cancel(
        &self,
        task_id: &str,
        result_out: &BroadcastSender<ServerMessage>,
        state_lock: &Arc<tokio::sync::Mutex<ShellState>>,
        state_path: &std::path::Path,
    ) -> bool {
        let mut guard = self.active.lock().await;
        let should_cancel = guard
            .as_ref()
            .is_some_and(|a| task_id.is_empty() || task_id == "screen" || a.id == task_id);
        if !should_cancel {
            return false;
        }
        let ask = guard.take().unwrap();
        ask.abort_handle.abort();
        self.reader.revoke();

        let mut st = state_lock.lock().await;
        st.context_read = Some(ContextReadRecord {
            method: crate::commands::method::CONTEXT_READ.to_string(),
            ms: shell_state::now_ms(),
            ok: false,
            channel: "screen".to_string(),
        });
        shell_state::write_state(state_path, &shell_state::render(&st));
        drop(st);

        let _ = result_out.send(ServerMessage::CommandResult {
            id: ask.id,
            ok: false,
            result: serde_json::json!({ "error": "cancelled_by_human" }),
        });
        true
    }
}

/// A [`ProviderSeam`] backed by the real [`friday_intelligence::InferenceFabric`].
///
/// Where a routed screen ask may go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenRoute {
    /// The user configured no provider: FRIDAY's defaults are theirs.
    Defaults,
    /// Only these of the user's providers (the ones that can see images).
    Among(Vec<String>),
    /// None of the user's providers can see images: send nothing, say why.
    Refuse(String),
}

/// A screenshot only goes to a provider the user chose. With providers
/// configured, a screen ask uses the ones that can see images and never falls
/// back to FRIDAY's defaults (keyless OpenCode Zen): if none can, it is
/// refused with what to do instead.
pub fn screen_route(user_providers: &[String], vision_providers: &[String]) -> ScreenRoute {
    if user_providers.is_empty() {
        return ScreenRoute::Defaults;
    }
    let allowed: Vec<String> = user_providers
        .iter()
        .filter(|p| vision_providers.contains(p))
        .cloned()
        .collect();
    if !allowed.is_empty() {
        return ScreenRoute::Among(allowed);
    }
    let mut msg = format!(
        "None of your AI providers ({}) can see images, so the screenshot was not sent anywhere.",
        user_providers.join(", ")
    );
    if user_providers.iter().any(|p| p == "llamacpp") {
        msg.push_str(
            " If your llama.cpp model can (llama-server --mmproj), set LLAMACPP_VISION=1.",
        );
    }
    msg.push_str(
        " To ask about the screen, add a provider that can see images \
         (friday setup-ai, or Choose AI provider in the tray on Windows).",
    );
    ScreenRoute::Refuse(msg)
}

/// Default deadline for screen context requests (45 s). Vision requests carrying
/// large PNG base64 payloads require extra transmission and visual token processing
/// time, whereas text asks use the daemon's 15 s deadline.
pub const DEFAULT_SCREEN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(45);

/// Sends the gated Screen payload to a model that can see images. With a
/// provider named (FRIDAY_CONTEXT_PROVIDER), that provider's vision model;
/// otherwise as [`screen_route`] decides. On a Windows dev host the daemon's
/// tests drive [`FakeCommandRunner`] + a scripted seam instead, so a passing
/// unit test is never mistaken for proof that real capture works.
#[derive(Clone)]
pub struct FabricProviderSeam {
    fabric: Arc<friday_intelligence::InferenceFabric>,
    provider: Option<String>,
}

impl FabricProviderSeam {
    /// `provider`: a provider to always use, or `None` to route.
    pub fn new(fabric: Arc<friday_intelligence::InferenceFabric>, provider: Option<&str>) -> Self {
        Self {
            fabric,
            provider: provider.map(str::to_string),
        }
    }
}

impl FabricProviderSeam {
    async fn route(
        &self,
        payload: &str,
        progress: Option<&(dyn for<'s> Fn(&'s str) + Send + Sync)>,
    ) -> Result<String, String> {
        if let Some(provider) = &self.provider {
            // A pinned provider's vision tier: answered whole.
            let text = self
                .fabric
                .complete_tier(
                    provider,
                    friday_intelligence::CapabilityTier::VisionFree,
                    payload,
                    false,
                )
                .await?;
            if let Some(p) = progress {
                p(&text);
            }
            return Ok(text);
        }
        let needs_vision = friday_intelligence::RequestRequirements {
            vision: friday_intelligence::Requirement::Required,
            ..Default::default()
        };
        let mut vision: Vec<String> = self
            .fabric
            .models
            .iter()
            .filter(|m| {
                m.capabilities
                    .contains(&friday_intelligence::ModelCapability::Vision)
            })
            .map(|m| m.provider.clone())
            .collect();
        vision.sort();
        vision.dedup();
        match screen_route(&self.fabric.user_providers(), &vision) {
            ScreenRoute::Defaults => match progress {
                Some(p) => {
                    self.fabric
                        .complete_with_progress(&needs_vision, payload, p)
                        .await
                }
                None => self.fabric.complete_with(&needs_vision, payload).await,
            },
            ScreenRoute::Among(providers) => {
                self.fabric
                    .complete_with_among(&needs_vision, &providers, payload, progress)
                    .await
            }
            ScreenRoute::Refuse(why) => Err(why),
        }
    }
}

#[async_trait]
impl ProviderSeam for FabricProviderSeam {
    async fn send(&self, payload: &str) -> Result<String, String> {
        self.route(payload, None).await
    }

    async fn send_streaming(
        &self,
        payload: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        self.route(payload, Some(progress)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_context_collector::{FakeCommandRunner, ScreenRegion};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

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

    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    /// The platform's capture command (grim on Linux, the Windows host on
    /// Windows), scripted to return a PNG.
    fn fake_collector() -> ContextCollector<FakeCommandRunner> {
        let full = friday_context_collector::screen_command(None);
        let region =
            friday_context_collector::screen_command(Some(ScreenRegion::new(0, 0, 1920, 1080)));
        let runner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout(&region.0, &strs(&region.1), PNG);
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
        assert_eq!(
            reader.provider.sent(),
            0,
            "no consent: nothing reaches the provider"
        );
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

    #[derive(Clone, Default)]
    struct RecordingCommandRunner {
        inner: FakeCommandRunner,
        call_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl friday_context_collector::CommandRunner for RecordingCommandRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
        ) -> Result<friday_context_collector::CommandOutput, friday_context_collector::RunnerError>
        {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.inner.run(program, args)
        }
    }

    #[tokio::test]
    async fn unconsented_screen_read_never_calls_capture() {
        let full = friday_context_collector::screen_command(None);
        let region =
            friday_context_collector::screen_command(Some(ScreenRegion::new(0, 0, 1920, 1080)));
        let inner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout(&region.0, &strs(&region.1), PNG);
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runner = RecordingCommandRunner {
            inner,
            call_count: call_count.clone(),
        };
        let collector = ContextCollector::new(runner);
        let reader = ScreenReader::new(collector, RecordingSeam::default(), "fabric");

        // Without consent, read must be refused BEFORE capturing
        assert!(!reader.is_consented());
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::NotAuthorized { .. }));
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "capture must NOT be called without consent"
        );

        // With consent, capture IS called
        reader.consent();
        let resp = reader.read().await.unwrap();
        assert_eq!(resp, "FRIDAY: I can see your screen.");
        assert!(
            call_count.load(Ordering::SeqCst) > 0,
            "capture must be called when consented"
        );
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

    #[tokio::test]
    async fn unused_consent_is_revoked_after_capture_failure() {
        let runner = FakeCommandRunner::new();
        let reader = ScreenReader::new(
            ContextCollector::new(runner),
            RecordingSeam::default(),
            "fabric",
        );
        reader.consent();
        assert!(reader.is_consented());
        let err = reader.read().await.unwrap_err();
        assert!(matches!(err, ContextReadError::Capture(_)));
        assert!(
            !reader.is_consented(),
            "capture failure must revoke unused consent"
        );
        assert_eq!(reader.provider.sent(), 0);

        // A reader sharing the same gate with a working collector cannot reach the provider
        let working_reader = ScreenReader {
            collector: fake_collector(),
            gate: reader.gate.clone(),
            egress: reader.egress.clone(),
            provider: reader.provider.clone(),
            provider_name: reader.provider_name.clone(),
            target: reader.target,
        };
        let second_err = working_reader.read().await.unwrap_err();
        assert!(matches!(second_err, ContextReadError::NotAuthorized { .. }));
        assert_eq!(working_reader.provider.sent(), 0);
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
    /// Answers in two steps through `send_streaming`.
    #[derive(Clone)]
    struct StreamingSeam;

    #[async_trait]
    impl ProviderSeam for StreamingSeam {
        async fn send(&self, _payload: &str) -> Result<String, String> {
            Ok("A terminal with an error.".to_string())
        }
        async fn send_streaming(
            &self,
            _payload: &str,
            progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            progress("A terminal");
            progress("A terminal with an error.");
            Ok("A terminal with an error.".to_string())
        }
    }

    #[tokio::test]
    async fn screen_answer_streams_through_the_reader() {
        let reader = ScreenReader::new(fake_collector(), StreamingSeam, "fabric");
        reader.consent();
        let seen = Mutex::new(Vec::<String>::new());
        let answer = reader
            .ask_streaming(Some("what is this?"), &|t: &str| {
                seen.lock().unwrap().push(t.to_string())
            })
            .await
            .unwrap();
        assert_eq!(answer, "A terminal with an error.");
        assert_eq!(
            seen.into_inner().unwrap(),
            ["A terminal", "A terminal with an error."]
        );
        // Without consent nothing streams and nothing is sent.
        let seen = Mutex::new(0);
        let blocked = reader
            .ask_streaming(None, &|_: &str| *seen.lock().unwrap() += 1)
            .await;
        assert!(blocked.is_err());
        assert_eq!(seen.into_inner().unwrap(), 0);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn screen_route_uses_defaults_only_without_a_user_provider() {
        assert_eq!(
            screen_route(&[], &names(&["opencode"])),
            ScreenRoute::Defaults
        );
    }

    #[test]
    fn screen_route_keeps_to_the_users_vision_providers() {
        let route = screen_route(
            &names(&["gemini", "llamacpp"]),
            &names(&["opencode", "gemini"]),
        );
        assert_eq!(route, ScreenRoute::Among(names(&["gemini"])));
    }

    #[test]
    fn screen_route_refuses_rather_than_fall_back_to_zen() {
        // A local model that can't see images: the screenshot must not go to
        // keyless Zen just because Zen can.
        match screen_route(&names(&["llamacpp"]), &names(&["opencode"])) {
            ScreenRoute::Refuse(why) => {
                assert!(why.contains("not sent anywhere"), "{why}");
                assert!(why.contains("LLAMACPP_VISION=1"), "{why}");
                assert!(why.contains("friday setup-ai"), "{why}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        match screen_route(&names(&["openrouter"]), &names(&["opencode"])) {
            ScreenRoute::Refuse(why) => assert!(!why.contains("LLAMACPP"), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

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

    #[derive(Clone)]
    struct BlockingSeam {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl BlockingSeam {
        fn new() -> (Self, Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
            let started = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            (
                Self {
                    started: started.clone(),
                    release: release.clone(),
                },
                started,
                release,
            )
        }
    }

    #[async_trait]
    impl ProviderSeam for BlockingSeam {
        async fn send(&self, payload: &str) -> Result<String, String> {
            self.send_streaming(payload, &|_| {}).await
        }

        async fn send_streaming(
            &self,
            _payload: &str,
            progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            progress("analyzing screen...");
            self.started.notify_one();
            self.release.notified().await;
            Ok("Screen analysis complete.".to_string())
        }
    }

    #[tokio::test]
    async fn second_concurrent_screen_ask_is_refused() {
        let (seam, started, release) = BlockingSeam::new();
        let reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
        let manager = ScreenAskManager::new(reader.clone());

        let (tx, mut rx) = tokio::sync::broadcast::channel(32);
        let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));
        let state = Arc::new(tokio::sync::Mutex::new(ShellState::starting("tok".into())));

        let cmd1 = Command {
            session_id: "s1".into(),
            id: "cmd-1".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "question": "what is this?" }),
        };

        // Start ask 1
        let res1 = manager
            .try_start(&cmd1, tx.clone(), state.clone(), state_path.clone())
            .await;
        assert!(res1.is_ok());
        assert!(manager.is_in_flight().await);

        // Wait until ask 1 has started streaming
        started.notified().await;

        // Try to start concurrent ask 2
        let cmd2 = Command {
            session_id: "s2".into(),
            id: "cmd-2".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "question": "second ask" }),
        };
        let res2 = manager
            .try_start(&cmd2, tx.clone(), state.clone(), state_path.clone())
            .await;
        assert!(res2.is_err());
        let err = res2.unwrap_err();
        match err {
            ServerMessage::CommandResult { id, ok, result } => {
                assert_eq!(id, "cmd-2");
                assert!(!ok);
                assert_eq!(result["error"], "FRIDAY is already looking at your screen");
            }
            _ => panic!("expected CommandResult"),
        }

        // Release ask 1
        release.notify_one();

        // Drain messages from ask 1: progress then CommandResult
        let mut got_result = false;
        while let Ok(msg) = rx.recv().await {
            match msg {
                ServerMessage::Event { topic, payload } => {
                    assert_eq!(topic, "context.progress");
                    assert_eq!(payload["id"], "cmd-1");
                }
                ServerMessage::CommandResult { id, ok, result } => {
                    assert_eq!(id, "cmd-1");
                    assert!(ok);
                    assert_eq!(result["response"], "Screen analysis complete.");
                    got_result = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(got_result);
        assert!(!manager.is_in_flight().await);
        let _ = std::fs::remove_file(&state_path);
    }

    #[derive(Clone)]
    struct PanickingSeam {
        panic_on_first: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl ProviderSeam for PanickingSeam {
        async fn send(&self, payload: &str) -> Result<String, String> {
            self.send_streaming(payload, &|_| {}).await
        }

        async fn send_streaming(
            &self,
            _payload: &str,
            _progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            if self.panic_on_first.swap(false, Ordering::SeqCst) {
                panic!("simulated provider panic in screen ask");
            }
            Ok("Screen analysis after panic succeeded.".to_string())
        }
    }

    #[tokio::test]
    async fn panicking_screen_ask_task_clears_active_and_revokes_consent() {
        let seam = PanickingSeam {
            panic_on_first: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        let reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
        let manager = ScreenAskManager::new(reader.clone());

        let (tx, mut rx) = tokio::sync::broadcast::channel(32);
        let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));
        let state = Arc::new(tokio::sync::Mutex::new(ShellState::starting("tok".into())));

        let cmd1 = Command {
            session_id: "s1".into(),
            id: "cmd-panic".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "question": "panic test" }),
        };

        // Start ask 1 which will panic inside the spawned task
        let res1 = manager
            .try_start(&cmd1, tx.clone(), state.clone(), state_path.clone())
            .await;
        assert!(res1.is_ok());

        // Wait a short moment for the task to run, panic, and drop guard to execute
        let mut cleared = false;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if !manager.is_in_flight().await {
                cleared = true;
                break;
            }
        }
        assert!(
            cleared,
            "manager.is_in_flight() must become false after task panic"
        );
        assert!(
            !reader.is_consented(),
            "consent must be revoked after task panic"
        );

        // Now a second ask MUST be accepted
        let cmd2 = Command {
            session_id: "s2".into(),
            id: "cmd-success".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "question": "second ask" }),
        };
        let res2 = manager
            .try_start(&cmd2, tx.clone(), state.clone(), state_path.clone())
            .await;
        assert!(
            res2.is_ok(),
            "second ask must be accepted after panic: {:?}",
            res2.err()
        );

        // Drain until cmd-success result is received
        let mut got_second = false;
        while let Ok(msg) = rx.recv().await {
            if let ServerMessage::CommandResult { id, ok, .. } = msg {
                if id == "cmd-success" {
                    assert!(ok);
                    got_second = true;
                    break;
                }
            }
        }
        assert!(got_second);
        assert!(!manager.is_in_flight().await);
        let _ = std::fs::remove_file(&state_path);
    }

    #[tokio::test]
    async fn task_cancel_stops_in_flight_screen_ask() {
        let (seam, started, _release) = BlockingSeam::new();
        let reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
        let manager = ScreenAskManager::new(reader.clone());

        let (tx, mut rx) = tokio::sync::broadcast::channel(32);
        let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));
        let state = Arc::new(tokio::sync::Mutex::new(ShellState::starting("tok".into())));

        let cmd = Command {
            session_id: "s1".into(),
            id: "ask-to-cancel".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "question": "will be cancelled" }),
        };

        let res = manager
            .try_start(&cmd, tx.clone(), state.clone(), state_path.clone())
            .await;
        assert!(res.is_ok());
        assert!(manager.is_in_flight().await);

        started.notified().await;

        // Cancel the ask
        let cancelled = manager
            .cancel("ask-to-cancel", &tx, &state, &state_path)
            .await;
        assert!(cancelled);
        assert!(!manager.is_in_flight().await);

        // Verify CommandResult arrives with cancelled shape
        let mut got_cancelled = false;
        while let Ok(msg) = rx.recv().await {
            if let ServerMessage::CommandResult { id, ok, result } = msg {
                assert_eq!(id, "ask-to-cancel");
                assert!(!ok);
                assert_eq!(result["error"], "cancelled_by_human");
                got_cancelled = true;
                break;
            }
        }
        assert!(got_cancelled);

        // State was updated to ok: false
        let st = state.lock().await;
        let cr = st.context_read.as_ref().expect("context_read recorded");
        assert!(!cr.ok);
        assert_eq!(cr.channel, "screen");

        // Consent was revoked
        assert!(!reader.is_consented());
        let _ = std::fs::remove_file(&state_path);
    }

    #[tokio::test]
    async fn task_cancel_with_unmatched_id_does_not_cancel() {
        let (seam, started, release) = BlockingSeam::new();
        let reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
        let manager = ScreenAskManager::new(reader.clone());

        let (tx, mut rx) = tokio::sync::broadcast::channel(32);
        let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));
        let state = Arc::new(tokio::sync::Mutex::new(ShellState::starting("tok".into())));

        let cmd = Command {
            session_id: "s1".into(),
            id: "my-ask".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true }),
        };

        assert!(manager
            .try_start(&cmd, tx.clone(), state.clone(), state_path.clone())
            .await
            .is_ok());
        started.notified().await;

        // Attempt cancel with mismatched id
        let cancelled = manager
            .cancel("unrelated-goal-id", &tx, &state, &state_path)
            .await;
        assert!(!cancelled);
        assert!(manager.is_in_flight().await);

        // Release and verify ask completes successfully
        release.notify_one();
        while let Ok(msg) = rx.recv().await {
            if let ServerMessage::CommandResult { id, ok, .. } = msg {
                assert_eq!(id, "my-ask");
                assert!(ok);
                break;
            }
        }
        let _ = std::fs::remove_file(&state_path);
    }

    #[tokio::test]
    async fn fabric_provider_seam_pinned_provider_honors_timeout_with_mock_server_that_never_answers(
    ) {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                // Accept connection and sleep: never reply
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                drop(socket);
            }
        });

        let mut fabric = friday_intelligence::InferenceFabric::new();
        fabric.request_timeout = Some(std::time::Duration::from_millis(150));
        let provider_name = "mock_pinned_vision";
        fabric.register_provider(Box::new(friday_intelligence::OpenAiProvider {
            name: provider_name.to_string(),
            base_url: format!("http://{addr}/v1"),
            api_key: String::new(),
        }));
        fabric.register_model(friday_intelligence::ModelInfo {
            id: "mock_model".to_string(),
            family: "vision".to_string(),
            provider: provider_name.to_string(),
            latency_ms: 10,
            reliability: 1.0,
            capabilities: vec![friday_intelligence::ModelCapability::Vision],
            max_context: 4096,
            tier: Some(friday_intelligence::CapabilityTier::VisionFree),
        });

        let seam = FabricProviderSeam::new(Arc::new(fabric), Some(provider_name));
        let payload = "SCREEN_CAPTURE mime=image/png geometry=fullscreen bytes=3 data=QUJD\nQUESTION: what is this?";
        let start = std::time::Instant::now();
        let err = seam
            .send(payload)
            .await
            .expect_err("must timeout when server hangs");
        let elapsed = start.elapsed();

        assert!(
            err.contains("timed out"),
            "expected timeout error message, got: {err}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "call must fail within deadline (~150ms), took {elapsed:?}"
        );
    }
}
