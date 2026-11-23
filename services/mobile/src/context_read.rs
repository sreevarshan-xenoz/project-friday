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
use friday_context_collector::{CaptureOptions, CapturedContext, ContextCollector, ScreenRegion};
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

    /// Send a plain-text prompt (no image) to any model that can answer it.
    /// A seam with one route sends it the same way as a screenshot.
    async fn send_text_streaming(
        &self,
        payload: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        self.send_streaming(payload, progress).await
    }
}

/// The words in the focused window, read from its accessibility tree, for a
/// screen ask that needs no screenshot. `class` is the compositor's name for
/// the app. `None` when there is no tree; too few words (under
/// [`MIN_SCREEN_TEXT`]) send a screenshot instead, once they are checked
/// for secrets.
#[async_trait]
pub trait ScreenText: Send + Sync {
    /// The words of `class`'s window titled `title` (the one in front), never
    /// of its other windows.
    async fn window_text(&self, class: &str, title: &str) -> Option<String>;
}

/// The words in a picture of the screen (an area the person picked), read on
/// this computer. Too few to answer from (under [`MIN_AREA_TEXT`]) and the
/// picture goes instead, once they are checked for secrets.
#[async_trait]
pub trait ImageText: Send + Sync {
    async fn image_text(&self, png: &[u8]) -> Option<String>;
}

/// An area with fewer letters and digits than this goes as a picture.
const MIN_AREA_TEXT: usize = 40;
/// A window whose tree gives less than this is asked about with a screenshot.
pub const MIN_SCREEN_TEXT: usize = 200;

/// Whether these words, read from what a screenshot would show, hold a key
/// or password: the picture then never leaves (`screen-secret-check`).
fn shows_secret(words: &str) -> bool {
    crate::features::on("screen-secret-check") && friday_goal_execution::mask_secrets(words).1 > 0
}

fn secret_on_screen() -> ContextReadError {
    ContextReadError::EgressBlocked {
        reason: "the screen shows what looks like a key or password".into(),
    }
}
/// The most text of one area sent to a model.
const MAX_AREA_TEXT: usize = 12_000;

/// [`ImageText`] read on this computer from the PNG: `tesseract` on Linux,
/// Windows' own OCR through FRIDAY's host (`friday-windows-host --ocr`).
pub struct LocalOcr {
    program: String,
    args: Vec<&'static str>,
}

impl LocalOcr {
    pub fn for_this_os() -> Self {
        if cfg!(windows) {
            Self {
                program: friday_context_collector::collector::windows_host_program(),
                args: vec!["--ocr"],
            }
        } else {
            Self {
                program: "tesseract".into(),
                args: vec!["stdin", "stdout"],
            }
        }
    }
}

#[async_trait]
impl ImageText for LocalOcr {
    async fn image_text(&self, png: &[u8]) -> Option<String> {
        use tokio::io::AsyncWriteExt;
        let mut command = tokio::process::Command::new(&self.program);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let write = async move {
            stdin.write_all(png).await.ok();
        };
        // Past this the picture goes instead.
        let (_, out) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            futures::future::join(write, child.wait_with_output()),
        )
        .await
        .ok()?;
        let out = out.ok().filter(|o| o.status.success())?;
        Some(area_words(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// Do two areas of the screen share any pixels?
fn overlaps(a: ScreenRegion, b: ScreenRegion) -> bool {
    let (ax2, ay2) = (a.x as i64 + a.width as i64, a.y as i64 + a.height as i64);
    let (bx2, by2) = (b.x as i64 + b.width as i64, b.y as i64 + b.height as i64);
    (a.x as i64) < bx2 && (b.x as i64) < ax2 && (a.y as i64) < by2 && (b.y as i64) < ay2
}

/// OCR output as the words to send: blank lines dropped, cut to
/// [`MAX_AREA_TEXT`].
fn area_words(raw: &str) -> String {
    let text: String = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    text.chars().take(MAX_AREA_TEXT).collect()
}

/// Enough of an area's words to answer from.
fn enough_area_words(words: &str) -> bool {
    words.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_AREA_TEXT
}

/// The line a [`ScreenText`] ends the words with when the window had more
/// than it sends.
pub const SCREEN_TEXT_CUT: &str = "[… the rest of the window was cut]";

/// What a screen ask sent to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSent {
    /// The window's own text; `cut` when only its first part fitted.
    Text { cut: bool },
    /// A screenshot of the window; `text_failed` when the text went first
    /// and the provider didn't answer it.
    Screenshot { text_failed: bool },
    /// A screenshot of the area the person picked.
    Area,
    /// The words in the area the person picked, read on this computer.
    AreaText,
    /// The window's words read off its screenshot on this computer (its app
    /// gave no text of its own): the picture stayed here.
    ScreenOcr,
}

impl ScreenSent {
    /// The "Used:" entry the answer shows.
    pub fn used(self) -> &'static str {
        match self {
            ScreenSent::Text { cut: false } => "the window's text",
            ScreenSent::Text { cut: true } => "the window's text (only its first part)",
            ScreenSent::Screenshot { text_failed: false } => "a screenshot of the window",
            ScreenSent::Screenshot { text_failed: true } => {
                "a screenshot of the window (the text went first and got no answer)"
            }
            ScreenSent::Area => "a screenshot of the area you picked",
            ScreenSent::AreaText => "the text in the area you picked (read on this computer)",
            ScreenSent::ScreenOcr => "the window's text, read from the screen on this computer",
        }
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
    /// The window in front is private (a password manager, a private browser
    /// window, a bank, the person's own list): nothing is read or sent.
    PrivateWindow,
}

impl std::fmt::Display for ContextReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextReadError::Capture(m) => write!(f, "screen capture failed: {m}"),
            ContextReadError::InvalidPayload(m) => write!(f, "invalid screen payload: {m}"),
            ContextReadError::NotAuthorized { reason, .. } => write!(f, "not authorized: {reason}"),
            ContextReadError::EgressBlocked { reason } => write!(f, "egress blocked: {reason}"),
            ContextReadError::Provider(m) => write!(f, "provider error: {m}"),
            ContextReadError::PrivateWindow => write!(f, "the window in front is private"),
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
    text: Option<Arc<dyn ScreenText>>,
    area_text: Option<Arc<dyn ImageText>>,
    /// Whether the window's words may go instead of a screenshot (screen text
    /// on). Off, they are still read here, only to check for a key.
    text_route: bool,
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
            text: None,
            area_text: None,
            text_route: true,
        }
    }

    /// Screen text off: the window's words never go to a model; they are
    /// read only to keep a screenshot that shows a key from leaving.
    pub fn check_only(mut self) -> Self {
        self.text_route = false;
        self
    }

    /// Answer from the focused window's accessibility text, when it has
    /// enough, instead of a screenshot.
    pub fn with_text(mut self, text: Arc<dyn ScreenText>) -> Self {
        self.text = Some(text);
        self
    }

    /// Answer about a picked area from its words (read locally), when it
    /// has enough, instead of the picture.
    pub fn with_area_text(mut self, ocr: Arc<dyn ImageText>) -> Self {
        self.area_text = Some(ocr);
        self
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
        self.ask_region_streaming(question, None, progress).await
    }

    /// [`ScreenReader::ask_streaming`] about one area of the screen (the
    /// person dragged it out), instead of the focused window.
    pub async fn ask_region_streaming(
        &self,
        question: Option<&str>,
        region: Option<ScreenRegion>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, ContextReadError> {
        self.ask_routed(question, region, progress)
            .await
            .map(|(answer, _)| answer)
    }

    /// [`ScreenReader::ask_region_streaming`], also saying what went to the
    /// model: the window's text or a screenshot.
    pub async fn ask_routed(
        &self,
        question: Option<&str>,
        region: Option<ScreenRegion>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<(String, ScreenSent), ContextReadError> {
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

        // Every host and client gets this, not only FRIDAY's own CLI: a
        // private window is neither read nor captured, nor is an area that
        // covers it (or might: its place unknown).
        if let Ok(CapturedContext::AppInfo {
            class,
            title,
            geometry,
            ..
        }) = self
            .collector
            .capture(ContextChannel::App, &CaptureOptions::default())
        {
            let covered = match (region, geometry) {
                (Some(r), Some(w)) => overlaps(r, w),
                _ => true,
            };
            if covered && crate::recall::private(&class, &title, &crate::recall::own_list()) {
                self.revoke();
                return Err(ContextReadError::PrivateWindow);
            }
        }

        let mut text_failed = false;
        if region.is_none() {
            let front = self.window_in_front();
            let mut secret = false;
            let from_text = self.ask_from_text(question, progress, &mut secret).await;
            // The text went masked; a picture can't be: it never goes.
            if secret && !matches!(from_text, Some(Ok(_))) {
                self.revoke();
                return Err(secret_on_screen());
            }
            match from_text {
                Some(Ok((answer, cut))) => return Ok((answer, ScreenSent::Text { cut })),
                // A busy or refusing provider on the text route: the
                // screenshot (consent was given for this ask) rather than no
                // answer. Every other refusal stands.
                // Only of the same window: one the person switched to while
                // the text was out (perhaps a private one) is never captured.
                Some(Err(ContextReadError::Provider(e))) => {
                    if front.is_none() || self.window_in_front() != front {
                        self.revoke();
                        return Err(ContextReadError::Provider(e));
                    }
                    tracing::warn!(
                        "[context.read] the text route failed ({e}); trying a screenshot"
                    );
                    text_failed = true;
                }
                Some(Err(e)) => return Err(e),
                None => {}
            }
        }

        let captured = match self.collector.capture(
            ContextChannel::Screen,
            &CaptureOptions {
                fullscreen: region.is_none(),
                // Just the window being asked about (the whole screen when
                // there is none), unless an area was picked.
                focused_window: region.is_none(),
                region,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                self.revoke();
                return Err(ContextReadError::Capture(e.to_string()));
            }
        };

        // A picked area, or a window whose app gave no text (Electron, a
        // canvas, a remote desktop): its words first, read here; the picture
        // only when there are too few, or the provider didn't answer them.
        // The words are read here even with the switch off, when they are
        // only checked for a secret.
        let words_on = if region.is_some() {
            crate::features::on("area-text")
        } else {
            // A window's own text the provider didn't answer: the picture.
            crate::features::on("screen-ocr") && !text_failed
        };
        if let (Some(ocr), CapturedContext::Image { bytes, .. }) = (
            self.area_text
                .as_ref()
                .filter(|_| words_on || crate::features::on("screen-secret-check")),
            &captured,
        ) {
            let words = ocr.image_text(bytes).await;
            let secret = words.as_deref().is_some_and(shows_secret);
            let words = words.filter(|w| self.text_route && words_on && enough_area_words(w));
            if let Some(words) = words {
                let intro = match region {
                    Some(r) => format!(
                        "The person picked an area of their screen ({}). Its text, read \
                         from the screen on their computer, follows between the lines.",
                        r.slurp()
                    ),
                    None => "The person asked about the window in front. Its app gives \
                             no text of its own, so its words were read from the screen \
                             on their computer (OCR: layout lost, a word may be misread); \
                             they follow between the lines."
                        .to_string(),
                };
                let route = if region.is_some() {
                    ScreenSent::AreaText
                } else {
                    ScreenSent::ScreenOcr
                };
                // The picture goes after a failed text only when nothing of
                // an answer was shown yet: never two answers in one.
                let streamed = std::sync::atomic::AtomicBool::new(false);
                let watch = |chunk: &str| {
                    streamed.store(true, std::sync::atomic::Ordering::Relaxed);
                    progress(chunk);
                };
                match self.send_words(&intro, &words, 0, question, &watch).await {
                    Ok(answer) => return Ok((answer, route)),
                    Err(ContextReadError::Provider(e))
                        if streamed.load(std::sync::atomic::Ordering::Relaxed) =>
                    {
                        self.revoke();
                        return Err(ContextReadError::Provider(e));
                    }
                    Err(ContextReadError::Provider(e)) => {
                        tracing::warn!(
                            "[context.read] the area's text got no answer ({e}); sending the picture"
                        );
                    }
                    Err(e) => {
                        self.revoke();
                        return Err(e);
                    }
                }
            }
            if secret {
                self.revoke();
                return Err(secret_on_screen());
            }
        }

        let mut payload = match screen_payload(&captured) {
            Ok(p) => p,
            Err(e) => {
                self.revoke();
                return Err(e);
            }
        };
        let mut masked_count = 0usize;
        if let Some(q) = question.map(str::trim).filter(|q| !q.is_empty()) {
            // One line: the provider reads everything after `QUESTION: `.
            let (masked_q, count) = friday_goal_execution::mask_secrets(q);
            masked_count = count;
            payload.push_str("\nQUESTION: ");
            payload.push_str(&masked_q.replace(['\n', '\r'], " "));
        }

        let composed = self
            .egress
            .process(&self.provider_name, &payload)
            .map_err(|reason| ContextReadError::EgressBlocked { reason })?;

        let current_ctx = friday_intelligence::current_egress_context();
        let egress_ctx = if current_ctx.masked.is_none() {
            friday_intelligence::EgressContext {
                goal_id: current_ctx.goal_id,
                masked: Some(masked_count),
            }
        } else {
            current_ctx
        };

        friday_intelligence::with_egress_context(
            egress_ctx,
            self.provider.send_streaming(&composed, progress),
        )
        .await
        .map(|answer| {
            let route = if region.is_some() {
                ScreenSent::Area
            } else {
                ScreenSent::Screenshot { text_failed }
            };
            (answer, route)
        })
        .map_err(ContextReadError::Provider)
    }

    /// The window in front (its app and title), if the desktop says.
    fn window_in_front(&self) -> Option<(String, String)> {
        match self
            .collector
            .capture(ContextChannel::App, &CaptureOptions::default())
        {
            Ok(CapturedContext::AppInfo { class, title, .. }) => Some((class, title)),
            _ => None,
        }
    }

    /// The focused window's words, when its app has an accessibility tree
    /// with enough in it: they go to the model as text (secrets masked, then
    /// the same egress check), and nothing is captured. `None` means use a
    /// screenshot.
    async fn ask_from_text(
        &self,
        question: Option<&str>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        secret: &mut bool,
    ) -> Option<Result<(String, bool), ContextReadError>> {
        let source = self.text.as_ref()?;
        let CapturedContext::AppInfo { class, title, .. } = self
            .collector
            .capture(ContextChannel::App, &CaptureOptions::default())
            .ok()?
        else {
            return None;
        };
        let words = source.window_text(&class, &title).await?;
        // The title too: a browser's address or a terminal's prompt is in the picture.
        *secret = shows_secret(&words) || shows_secret(&title);
        if !self.text_route || words.chars().count() < MIN_SCREEN_TEXT {
            return None;
        }
        let cut = words.ends_with(SCREEN_TEXT_CUT);
        // A title can carry a token too (a URL with a key in it, a terminal).
        let (title, masked_title) = friday_goal_execution::mask_secrets(&title);
        let title = title.replace(['\n', '\r'], " ");
        let intro = format!(
            "The person asked about the window they are looking at ({class}: {title}). \
             Its text, read from the app's accessibility tree, follows between the lines."
        );
        Some(
            self.send_words(&intro, &words, masked_title, question, progress)
                .await
                .map(|answer| (answer, cut)),
        )
    }

    /// Send screen words as a text prompt: `intro`, the words (secrets
    /// masked) between lines, then the question, through the egress check.
    /// `masked_before` counts secrets already masked in `intro`.
    async fn send_words(
        &self,
        intro: &str,
        words: &str,
        masked_before: usize,
        question: Option<&str>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, ContextReadError> {
        let (words, masked_words) = friday_goal_execution::mask_secrets(words);
        let question = question.map(str::trim).filter(|q| !q.is_empty());
        let (task, masked_q) = match question {
            Some(q) => {
                let (m, n) = friday_goal_execution::mask_secrets(q);
                (m.replace(['\n', '\r'], " "), n)
            }
            None => ("Describe what the person is looking at.".to_string(), 0),
        };
        let payload = format!("{intro}\n---\n{words}\n---\nQUESTION: {task}");
        let composed = self
            .egress
            .process(&self.provider_name, &payload)
            .map_err(|reason| ContextReadError::EgressBlocked { reason })?;
        let current = friday_intelligence::current_egress_context();
        let ctx = if current.masked.is_none() {
            friday_intelligence::EgressContext {
                goal_id: current.goal_id,
                masked: Some(masked_words + masked_q + masked_before),
            }
        } else {
            current
        };
        friday_intelligence::with_egress_context(
            ctx,
            self.provider.send_text_streaming(&composed, progress),
        )
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
    /// Where an answered screen ask is noted for its conversation
    /// (`conversation.turn`), so the next ask can follow on from it.
    bus: Option<friday_core::event::EventBus>,
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
            bus: None,
        }
    }

    /// Screen asks join the conversation they name (`conversation` in the
    /// request): the question and the answer, never the window's text.
    pub fn with_bus(mut self, bus: friday_core::event::EventBus) -> Self {
        self.bus = Some(bus);
        self
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

        let question = cmd
            .params
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let conversation = cmd
            .params
            .get("conversation")
            .and_then(|v| v.as_str())
            .filter(|c| {
                (1..=64).contains(&c.len())
                    && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            })
            .map(str::to_string);
        let bus = self.bus.clone();
        let region = match cmd.params.get("region") {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => match region_param(v) {
                Some(r) => Some(r),
                None => {
                    return Err(ServerMessage::CommandResult {
                        id: cmd.id.clone(),
                        ok: false,
                        result: serde_json::json!({ "error": "region must be {x, y, w, h} with w and h above 0" }),
                    })
                }
            },
        };
        // Consent is given last: a request refused above must not leave it switched on.
        let want_consent = cmd
            .params
            .get("consent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if want_consent {
            self.reader.consent();
        }
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

            let egress_ctx = friday_intelligence::EgressContext {
                goal_id: Some(task_cmd_id.clone()),
                masked: None,
            };
            let asked = friday_intelligence::with_egress_context(
                egress_ctx,
                reader.ask_routed(question.as_deref(), region, &progress),
            )
            .await;

            let (ok, result) = match asked {
                Ok((response, route)) => {
                    tracing::info!("[context.read] screen delivered to provider");
                    if let (Some(bus), Some(session)) = (&bus, &conversation) {
                        let _ = bus
                            .publish(friday_core::event::Event::new(
                                "conversation.turn",
                                "context",
                                serde_json::json!({
                                    "session_id": session,
                                    "goal": format!(
                                        "(about the window on screen) {}",
                                        question.as_deref().unwrap_or("what is this?")
                                    ),
                                    "result": response,
                                }),
                            ))
                            .await;
                    }
                    (
                        true,
                        serde_json::json!({ "response": response, "used": [route.used()] }),
                    )
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
        if self.fabric.private_mode {
            let provider = self.provider.as_deref().unwrap_or("llamacpp");
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
        // The model picked for screen questions goes first, if it can see.
        let prefer = crate::model_jobs::pick("screen");
        let prefer = prefer.as_deref();
        match screen_route(&self.fabric.user_providers(), &vision) {
            ScreenRoute::Defaults => {
                self.fabric
                    .complete_preferring(&needs_vision, prefer, None, payload, progress)
                    .await
            }
            ScreenRoute::Among(providers) => {
                self.fabric
                    .complete_preferring(&needs_vision, prefer, Some(&providers), payload, progress)
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

    /// Words only: any model the person's providers (or FRIDAY's defaults)
    /// offer can answer, whether or not it can see images.
    async fn send_text_streaming(
        &self,
        payload: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        if self.fabric.private_mode || self.provider.is_some() {
            let provider = self.provider.as_deref().unwrap_or("llamacpp");
            let text = self
                .fabric
                .complete_tier(
                    provider,
                    friday_intelligence::CapabilityTier::ConversationFast,
                    payload,
                    false,
                )
                .await?;
            progress(&text);
            return Ok(text);
        }
        self.fabric
            .complete_with_progress(
                &friday_intelligence::RequestRequirements::default(),
                payload,
                progress,
            )
            .await
    }
}

/// A `context.read` request's `region`: `{x, y, w, h}` in desktop pixels
/// (slurp's coordinates, so any monitor), with a non-empty size.
fn region_param(v: &serde_json::Value) -> Option<ScreenRegion> {
    let int = |k: &str| v.get(k).and_then(|n| n.as_i64());
    let (x, y, w, h) = (int("x")?, int("y")?, int("w")?, int("h")?);
    let fits = |n: i64| (-100_000..=100_000).contains(&n);
    if !(fits(x) && fits(y)) || !(1..=100_000).contains(&w) || !(1..=100_000).contains(&h) {
        return None;
    }
    Some(ScreenRegion::new(x as i32, y as i32, w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use friday_context_collector::FakeCommandRunner;
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
            text: None,
            area_text: None,
            text_route: true,
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
    async fn a_refused_screen_ask_does_not_leave_consent_on() {
        let (seam, _started, _release) = BlockingSeam::new();
        let reader = Arc::new(ScreenReader::new(fake_collector(), seam, "fabric"));
        let manager = ScreenAskManager::new(reader.clone());
        let (tx, _rx) = tokio::sync::broadcast::channel(32);
        let state_path = std::env::temp_dir().join(format!("state-{}.json", uuid::Uuid::new_v4()));
        let state = Arc::new(tokio::sync::Mutex::new(ShellState::starting("tok".into())));

        let cmd = Command {
            session_id: "s1".into(),
            id: "bad-region".into(),
            method: crate::commands::method::CONTEXT_READ.into(),
            params: serde_json::json!({ "consent": true, "region": { "x": 0, "y": 0, "w": 0, "h": 0 } }),
        };
        assert!(manager
            .try_start(&cmd, tx, state, state_path)
            .await
            .is_err());
        assert!(!reader.is_consented());
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
        // Its egress log in a temp file, never the person's own.
        fabric.egress_log_path = std::env::temp_dir().join("friday-test-egress.jsonl");
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

    #[test]
    fn region_param_takes_a_box_and_refuses_nonsense() {
        let r =
            region_param(&serde_json::json!({"x": -1920, "y": 40, "w": 640, "h": 360})).unwrap();
        assert_eq!((r.x, r.y, r.width, r.height), (-1920, 40, 640, 360));
        for bad in [
            serde_json::json!({"x": 0, "y": 0, "w": 0, "h": 10}),
            serde_json::json!({"x": 0, "y": 0, "w": 10}),
            serde_json::json!({"x": "a", "y": 0, "w": 10, "h": 10}),
            serde_json::json!({"x": 0, "y": 0, "w": -5, "h": 10}),
        ] {
            assert!(region_param(&bad).is_none(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_picked_area_is_captured_instead_of_the_window() {
        let area = ScreenRegion::new(10, 20, 300, 200);
        let cmd = friday_context_collector::screen_command(Some(area));
        let runner = FakeCommandRunner::new().script_stdout(&cmd.0, &strs(&cmd.1), PNG);
        let reader = ScreenReader::new(
            ContextCollector::new(runner),
            RecordingSeam::default(),
            "fabric",
        );
        reader.consent();
        reader
            .ask_region_streaming(Some("what is this?"), Some(area), &|_: &str| {})
            .await
            .unwrap();
        assert!(reader.provider.last().unwrap().contains("SCREEN_CAPTURE"));
    }

    #[tokio::test]
    async fn fabric_provider_seam_private_mode_fails_when_no_local_vision_model() {
        let mut fabric = friday_intelligence::InferenceFabric::new().with_private_mode(true);
        fabric.egress_log_path = std::env::temp_dir().join("friday-test-egress.jsonl");
        let fabric = fabric;
        let seam = FabricProviderSeam::new(Arc::new(fabric), None);
        let payload =
            "SCREEN_CAPTURE mime=image/png geometry=fullscreen bytes=3 data=QUJD\nQUESTION: test";
        let err = seam.send(payload).await.unwrap_err();
        assert_eq!(
            err,
            "Private mode is on and no local model is running (start it: friday private on)"
        );
    }

    struct FakeText(Option<String>);

    #[async_trait]
    impl ScreenText for FakeText {
        async fn window_text(&self, class: &str, title: &str) -> Option<String> {
            assert_eq!((class, title), ("nautilus", "Home"));
            // Long enough to go as text (a window with a real tree).
            self.0
                .clone()
                .map(|t| format!("{}\n{t}", "row: item ".repeat(MIN_SCREEN_TEXT / 10)))
        }
    }

    fn collector_with_window() -> ContextCollector<FakeCommandRunner> {
        let full = friday_context_collector::screen_command(None);
        let win = friday_context_collector::window_command();
        let shot =
            friday_context_collector::screen_command(Some(ScreenRegion::new(0, 0, 800, 600)));
        let runner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout(&shot.0, &strs(&shot.1), PNG)
            .script_stdout(
                &win.0,
                &strs(&win.1),
                br#"{"class":"nautilus","title":"Home","at":[0,0],"size":[800,600]}"#,
            );
        ContextCollector::new(runner)
    }

    /// A provider that refuses text and answers screenshots.
    #[derive(Clone, Default)]
    struct TextRefused(RecordingSeam);

    #[async_trait]
    impl ProviderSeam for TextRefused {
        async fn send(&self, payload: &str) -> Result<String, String> {
            self.0.send(payload).await
        }
        async fn send_text_streaming(
            &self,
            _: &str,
            _: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            Err("rate limited".to_string())
        }
    }

    #[tokio::test]
    async fn a_busy_provider_on_the_text_falls_back_to_the_screenshot() {
        let seam = TextRefused::default();
        let reader = ScreenReader::new(collector_with_window(), seam.clone(), "fabric")
            .with_text(Arc::new(FakeText(Some("Report.pdf\nbutton: Open".into()))));
        reader.consent();
        let (answer, sent) = reader
            .ask_routed(Some("what is here?"), None, &|_| {})
            .await
            .unwrap();
        assert!(answer.contains("FRIDAY"));
        assert_eq!(sent, ScreenSent::Screenshot { text_failed: true });
        assert!(
            seam.0.last().unwrap().contains("SCREEN_CAPTURE"),
            "the screenshot went instead"
        );
        assert!(sent.used().contains("the text went first"));
    }

    #[tokio::test]
    async fn no_fallback_screenshot_of_a_window_switched_to_meanwhile() {
        // The window in front changes between the text and the fallback.
        let win = friday_context_collector::window_command();
        let full = friday_context_collector::screen_command(None);
        let runner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout_seq(
                &win.0,
                &strs(&win.1),
                &[
                    // The private check, the window before, the text's own look.
                    br#"{"class":"nautilus","title":"Home","at":[0,0],"size":[800,600]}"#
                        .as_slice(),
                    br#"{"class":"nautilus","title":"Home","at":[0,0],"size":[800,600]}"#
                        .as_slice(),
                    br#"{"class":"nautilus","title":"Home","at":[0,0],"size":[800,600]}"#
                        .as_slice(),
                    br#"{"class":"keepassxc","title":"Passwords","at":[0,0],"size":[800,600]}"#
                        .as_slice(),
                ],
            );
        let seam = TextRefused::default();
        let reader = ScreenReader::new(ContextCollector::new(runner), seam.clone(), "fabric")
            .with_text(Arc::new(FakeText(Some("Report.pdf".into()))));
        reader.consent();
        let got = reader.ask_routed(None, None, &|_| {}).await;
        assert!(matches!(got, Err(ContextReadError::Provider(_))), "{got:?}");
        assert_eq!(seam.0.sent(), 0, "no screenshot of the other window");
    }

    #[tokio::test]
    async fn the_answer_says_what_was_sent() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some(format!(
                "Report.pdf\n{SCREEN_TEXT_CUT}"
            )))));
        reader.consent();
        let (_, sent) = reader.ask_routed(None, None, &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Text { cut: true });
        assert_eq!(sent.used(), "the window's text (only its first part)");
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric");
        reader.consent();
        let (_, sent) = reader.ask_routed(None, None, &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Screenshot { text_failed: false });
    }

    #[tokio::test]
    async fn a_private_window_is_neither_read_nor_captured() {
        struct NeverRead;
        #[async_trait]
        impl ScreenText for NeverRead {
            async fn window_text(&self, _: &str, _: &str) -> Option<String> {
                panic!("a private window's text was read")
            }
        }
        let win = friday_context_collector::window_command();
        let full = friday_context_collector::screen_command(None);
        let runner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout(
                &win.0,
                &strs(&win.1),
                br#"{"class":"firefox","title":"Private Browsing - Mozilla Firefox","at":[0,0],"size":[800,600]}"#,
            );
        let reader = ScreenReader::new(
            ContextCollector::new(runner),
            RecordingSeam::default(),
            "fabric",
        )
        .with_text(Arc::new(NeverRead));
        reader.consent();
        let got = reader.ask(Some("what is this?")).await;
        assert_eq!(got, Err(ContextReadError::PrivateWindow));
        assert!(
            reader.provider.last().is_none(),
            "nothing reaches the model"
        );
        // The consent is used up: the next ask needs a new one.
        assert!(matches!(
            reader.ask(None).await,
            Err(ContextReadError::NotAuthorized { .. })
        ));
    }

    #[tokio::test]
    async fn a_window_with_words_is_asked_about_as_text_with_no_capture() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some("Report.pdf\nbutton: Open".into()))));
        reader.consent();
        let answer = reader.ask(Some("what files are here?")).await.unwrap();
        assert!(answer.contains("FRIDAY"));
        let sent = reader.provider.last().unwrap();
        assert!(sent.contains("Report.pdf"), "{sent}");
        assert!(sent.contains("QUESTION: what files are here?"), "{sent}");
        assert!(sent.contains("nautilus: Home"), "{sent}");
        assert!(!sent.contains("SCREEN_CAPTURE"), "no screenshot: {sent}");
    }

    #[tokio::test]
    async fn text_with_a_secret_is_masked_before_it_leaves() {
        let secret = format!("sk-{}", "a1B2c3D4e5F6g7H8i9J0k1L2");
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some(format!("token {secret} shown")))));
        reader.consent();
        reader.ask(Some("hi")).await.unwrap();
        let sent = reader.provider.last().unwrap();
        assert!(!sent.contains(&secret), "{sent}");
    }

    #[tokio::test]
    async fn a_window_without_words_falls_back_to_the_screenshot() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(None)));
        reader.consent();
        reader.ask(Some("what is this?")).await.unwrap();
        let sent = reader.provider.last().unwrap();
        assert!(sent.starts_with("SCREEN_CAPTURE"), "{sent}");
    }

    #[tokio::test]
    async fn a_picked_region_never_takes_the_window_text() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some("words".into()))));
        reader.consent();
        let region = Some(ScreenRegion::new(0, 0, 1920, 1080));
        // Not scripted for this region: the capture fails, proving the text
        // path was not taken.
        let _ = reader
            .ask_region_streaming(None, region, &|_: &str| {})
            .await;
        assert_eq!(
            reader
                .provider
                .last()
                .filter(|p| !p.starts_with("SCREEN_CAPTURE")),
            None
        );
    }

    struct FakeOcr(Option<String>);

    #[async_trait]
    impl ImageText for FakeOcr {
        async fn image_text(&self, png: &[u8]) -> Option<String> {
            assert_eq!(png, PNG, "the area's own picture is read");
            self.0.clone()
        }
    }

    const AREA_WORDS: &str = "error[E0308]: mismatched types\nexpected `u32`, found `&str`";

    #[tokio::test]
    async fn a_picked_area_with_words_sends_the_words() {
        let area = ScreenRegion::new(0, 0, 800, 600);
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(AREA_WORDS.into()))));
        reader.consent();
        let (_, sent) = reader
            .ask_routed(Some("why?"), Some(area), &|_| {})
            .await
            .unwrap();
        assert_eq!(sent, ScreenSent::AreaText);
        let payload = reader.provider.last().unwrap();
        assert!(!payload.contains("SCREEN_CAPTURE"), "{payload}");
        assert!(payload.contains("mismatched types") && payload.ends_with("QUESTION: why?"));
        assert!(sent.used().contains("read on this computer"));
        assert!(!reader.is_consented(), "consent is spent");
    }

    #[tokio::test]
    async fn a_picked_area_with_few_words_sends_the_picture() {
        let area = ScreenRegion::new(0, 0, 800, 600);
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(None)));
        reader.consent();
        let (_, sent) = reader.ask_routed(None, Some(area), &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Area);
        assert!(reader
            .provider
            .last()
            .unwrap()
            .starts_with("SCREEN_CAPTURE"));
    }

    #[tokio::test]
    async fn a_busy_provider_on_the_area_words_gets_the_picture() {
        let area = ScreenRegion::new(0, 0, 800, 600);
        let seam = TextRefused::default();
        let reader = ScreenReader::new(collector_with_window(), seam.clone(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(AREA_WORDS.into()))));
        reader.consent();
        let (_, sent) = reader.ask_routed(None, Some(area), &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Area);
        assert!(seam.0.last().unwrap().starts_with("SCREEN_CAPTURE"));
    }

    /// An app with no accessibility text (Electron, a canvas, a remote
    /// desktop): its words are read from the screenshot on this computer and
    /// go instead of the picture, and the card names where they came from.
    #[tokio::test]
    async fn a_window_with_no_text_answers_from_words_read_off_the_screen() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(AREA_WORDS.into()))));
        reader.consent();
        let (_, sent) = reader
            .ask_routed(Some("why?"), None, &|_| {})
            .await
            .unwrap();
        assert_eq!(sent, ScreenSent::ScreenOcr);
        let payload = reader.provider.last().unwrap();
        assert!(!payload.contains("SCREEN_CAPTURE"), "{payload}");
        assert!(payload.contains("mismatched types") && payload.ends_with("QUESTION: why?"));
        assert!(sent
            .used()
            .contains("read from the screen on this computer"));
        assert!(!reader.is_consented(), "consent is spent");
    }

    #[tokio::test]
    async fn a_window_with_too_few_words_on_screen_sends_the_screenshot() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some("OK".into()))));
        reader.consent();
        let (_, sent) = reader.ask_routed(None, None, &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Screenshot { text_failed: false });
        assert!(reader
            .provider
            .last()
            .unwrap()
            .starts_with("SCREEN_CAPTURE"));
    }

    /// A window whose few words are given as they are (no padding).
    struct FewWords(&'static str);

    #[async_trait]
    impl ScreenText for FewWords {
        async fn window_text(&self, _: &str, _: &str) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    const ON_SCREEN_KEY: &str = "password: not-a-real-one";

    #[tokio::test]
    async fn a_key_on_screen_never_leaves_as_a_picture() {
        // Too few words for the text route: the screenshot would go, and
        // would show the key.
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FewWords(ON_SCREEN_KEY)));
        reader.consent();
        let got = reader
            .ask_routed(Some("what is this?"), None, &|_| {})
            .await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert_eq!(reader.provider.sent(), 0, "no screenshot went");
        assert!(!reader.is_consented());

        // A key in the title only (an address bar, a prompt): no picture.
        let full = friday_context_collector::screen_command(None);
        let win = friday_context_collector::window_command();
        let titled =
            format!(r#"{{"class":"foot","title":"{ON_SCREEN_KEY}","at":[0,0],"size":[800,600]}}"#);
        let runner = FakeCommandRunner::new()
            .script_stdout(&full.0, &strs(&full.1), PNG)
            .script_stdout(&win.0, &strs(&win.1), titled.as_bytes());
        let reader = ScreenReader::new(
            ContextCollector::new(runner),
            RecordingSeam::default(),
            "fabric",
        )
        .with_text(Arc::new(FewWords("a few words")));
        reader.consent();
        let got = reader.ask_routed(None, None, &|_| {}).await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert_eq!(reader.provider.sent(), 0);

        // The text route busy: no screenshot of the window instead.
        let seam = TextRefused::default();
        let reader = ScreenReader::new(collector_with_window(), seam.clone(), "fabric")
            .with_text(Arc::new(FakeText(Some(ON_SCREEN_KEY.into()))));
        reader.consent();
        let got = reader.ask_routed(None, None, &|_| {}).await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert!(!seam.0.last().unwrap_or_default().contains("SCREEN_CAPTURE"));

        // A picked area whose words show a key: neither its words nor its picture.
        let area = ScreenRegion::new(0, 0, 800, 600);
        let seam = TextRefused::default();
        let reader = ScreenReader::new(collector_with_window(), seam.clone(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(format!(
                "{AREA_WORDS}\n{ON_SCREEN_KEY}"
            )))));
        reader.consent();
        let got = reader.ask_routed(None, Some(area), &|_| {}).await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert!(!seam.0.last().unwrap_or_default().contains("SCREEN_CAPTURE"));
        // Too few of its words to send, and one is a key: no picture either.
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(ON_SCREEN_KEY.into()))));
        reader.consent();
        let got = reader.ask_routed(None, Some(area), &|_| {}).await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert_eq!(reader.provider.sent(), 0);
    }

    #[tokio::test]
    async fn screen_text_off_still_checks_for_a_key_but_sends_no_words() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some("Report.pdf".into()))))
            .check_only();
        reader.consent();
        let (_, sent) = reader.ask_routed(None, None, &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Screenshot { text_failed: false });
        assert!(!reader.provider.last().unwrap().contains("Report.pdf"));
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some(ON_SCREEN_KEY.into()))))
            .check_only();
        reader.consent();
        let got = reader.ask_routed(None, None, &|_| {}).await;
        assert!(
            matches!(got, Err(ContextReadError::EgressBlocked { .. })),
            "{got:?}"
        );
        assert_eq!(reader.provider.sent(), 0);
    }

    /// A private window at 0,0 800x600, and a capture of any area.
    fn collector_with_private_window(area: ScreenRegion) -> ContextCollector<FakeCommandRunner> {
        let win = friday_context_collector::window_command();
        let shot = friday_context_collector::screen_command(Some(area));
        let runner = FakeCommandRunner::new()
            .script_stdout(&shot.0, &strs(&shot.1), PNG)
            .script_stdout(
                &win.0,
                &strs(&win.1),
                br#"{"class":"firefox","title":"Private Browsing - Mozilla Firefox","at":[0,0],"size":[800,600]}"#,
            );
        ContextCollector::new(runner)
    }

    #[tokio::test]
    async fn an_area_over_a_private_window_is_refused() {
        let area = ScreenRegion::new(700, 500, 300, 200);
        let reader = ScreenReader::new(
            collector_with_private_window(area),
            RecordingSeam::default(),
            "fabric",
        );
        reader.consent();
        let got = reader.ask_routed(None, Some(area), &|_| {}).await;
        assert_eq!(got.map(|(_, s)| s), Err(ContextReadError::PrivateWindow));
        assert_eq!(reader.provider.sent(), 0);
        assert!(!reader.is_consented());
    }

    #[tokio::test]
    async fn an_area_beside_a_private_window_is_fine() {
        let area = ScreenRegion::new(800, 0, 300, 200);
        let reader = ScreenReader::new(
            collector_with_private_window(area),
            RecordingSeam::default(),
            "fabric",
        );
        reader.consent();
        let (_, sent) = reader.ask_routed(None, Some(area), &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::Area);
    }

    #[test]
    fn areas_overlap_only_when_they_share_pixels() {
        let w = ScreenRegion::new(0, 0, 800, 600);
        assert!(overlaps(ScreenRegion::new(799, 599, 10, 10), w));
        assert!(!overlaps(ScreenRegion::new(800, 0, 10, 10), w));
        assert!(!overlaps(ScreenRegion::new(-10, -10, 10, 10), w));
        assert!(overlaps(ScreenRegion::new(-1920, 0, 4000, 10), w));
    }

    #[tokio::test]
    async fn secrets_in_an_areas_words_are_masked() {
        let area = ScreenRegion::new(0, 0, 800, 600);
        let secret = format!("sk-{}", "a1B2c3D4e5F6g7H8i9J0k1L2");
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(format!(
                "{AREA_WORDS}\ntoken {secret} shown"
            )))));
        reader.consent();
        let (_, sent) = reader.ask_routed(None, Some(area), &|_| {}).await.unwrap();
        assert_eq!(sent, ScreenSent::AreaText);
        assert!(!reader.provider.last().unwrap().contains(&secret));
    }

    /// A provider that streams a bit of the text's answer, then fails.
    #[derive(Clone, Default)]
    struct DropsMidway(RecordingSeam);

    #[async_trait]
    impl ProviderSeam for DropsMidway {
        async fn send(&self, payload: &str) -> Result<String, String> {
            self.0.send(payload).await
        }
        async fn send_text_streaming(
            &self,
            _: &str,
            progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            progress("The error means");
            Err("connection reset".to_string())
        }
    }

    #[tokio::test]
    async fn an_area_answer_cut_off_midway_is_not_asked_again_with_the_picture() {
        let area = ScreenRegion::new(0, 0, 800, 600);
        let seam = DropsMidway::default();
        let reader = ScreenReader::new(collector_with_window(), seam.clone(), "fabric")
            .with_area_text(Arc::new(FakeOcr(Some(AREA_WORDS.into()))));
        reader.consent();
        let got = reader.ask_routed(None, Some(area), &|_| {}).await;
        assert!(matches!(got, Err(ContextReadError::Provider(_))), "{got:?}");
        assert_eq!(seam.0.sent(), 0, "no second answer from the picture");
        assert!(!reader.is_consented());
    }

    #[test]
    fn area_words_drops_blank_lines_and_refuses_too_few() {
        assert!(!enough_area_words(&area_words("  \n\x0c\nOK\n")));
        let many = "The quick brown fox\n\n   \njumps over the lazy dog again";
        assert_eq!(
            area_words(many),
            "The quick brown fox\njumps over the lazy dog again"
        );
        assert!(enough_area_words(&area_words(many)));
        assert_eq!(
            area_words(&"a".repeat(MAX_AREA_TEXT + 50)).len(),
            MAX_AREA_TEXT
        );
    }

    #[tokio::test]
    async fn text_without_consent_reads_nothing() {
        let reader = ScreenReader::new(collector_with_window(), RecordingSeam::default(), "fabric")
            .with_text(Arc::new(FakeText(Some("words".into()))));
        assert!(matches!(
            reader.ask(Some("x")).await,
            Err(ContextReadError::NotAuthorized { .. })
        ));
        assert_eq!(reader.provider.sent(), 0);
    }
}
