//! Real audio perception — the second genuine perception organ (MasterPlan #14).
//!
//! The constitution's missing piece: *"No VAD/ASR/diarization/turn detection …
//! not sensing."* This sensor runs the real pipeline:
//!
//! ```text
//! microphone (WASAPI shared-mode capture)
//!      ↓
//! i16 PCM frames
//!      ↓
//! energy VAD (RMS + hangover)   ← real, deterministic
//!      ↓
//! turn detector (speech run closed by silence)  ← real, deterministic
//!      ↓
//! ASR seam (SpeechToText trait) ← optional backend
//!      ↓
//! Topic::TurnDetected event → SensorLoop → GlobalWorkspace
//! ```
//!
//! Two honesty rules, mirroring the screen sensor:
//!
//! - **No ASR attached → no transcript.** A turn event carries
//!   `"transcript": null` and `"asr": null` — the pipeline never fabricates
//!   words it did not hear. Real ASR (Whisper et al.) plugs into the
//!   [`SpeechToText`] trait; nothing else changes.
//! - **No capture device → no events.** WASAPI open failure (headless,
//!   no default input, permission) degrades the sensor to silence — it never
//!   invents a turn.
//!
//! The VAD, turn detection, and event building are **pure and platform-neutral**
//! (tested everywhere, including end-to-end over synthetic PCM); only the
//! capture itself is Windows-gated. Small-specialist → event → core: only the
//! completed turn (a few hundred bytes of metadata) enters the workspace —
//! never raw PCM.

use std::sync::Arc;

use async_trait::async_trait;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;

use crate::Sensor;

/// PCM sample rate the sensor requests from WASAPI (and interprets synthetic
/// frames at).
pub const DEFAULT_SAMPLE_RATE: u32 = 16_000;

/// Samples per VAD/turn frame — 20 ms at 16 kHz.
pub const FRAME_SAMPLES: usize = 320;

/// Root-mean-square energy of an i16 mono frame, normalized to [0, 1].
/// `0.0` = digital silence. Pure.
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    (rms / i16::MAX as f64) as f32
}

/// Energy-based voice activity detector: `RMS ≥ threshold` is speech; once
/// speech is seen, a short `hangover_frames` of quiet still counts as speech
/// (so natural pauses inside a word/phrase don't split it). Pure + stateless
/// enough to be fully unit-tested.
#[derive(Debug, Clone)]
pub struct Vad {
    /// RMS (normalized) above which a frame is speech.
    pub threshold: f32,
    /// Consecutive sub-threshold frames tolerated while still "in speech".
    pub hangover_frames: usize,
    quiet_run: usize,
    in_speech: bool,
}

impl Default for Vad {
    fn default() -> Self {
        Self {
            threshold: 0.02,
            hangover_frames: 2,
            quiet_run: 0,
            in_speech: false,
        }
    }
}

impl Vad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            threshold,
            ..Self::default()
        }
    }

    /// Classify one frame's RMS. `true` = speech. The hangover **extends**
    /// speech after a loud frame but never **creates** it — leading silence
    /// stays silence, so quiet is not mistaken for speech.
    pub fn classify(&mut self, energy: f32) -> bool {
        if energy >= self.threshold {
            self.quiet_run = 0;
            self.in_speech = true;
            true
        } else if self.in_speech {
            self.quiet_run += 1;
            if self.quiet_run > self.hangover_frames {
                self.in_speech = false;
            }
            self.quiet_run <= self.hangover_frames
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.quiet_run = 0;
        self.in_speech = false;
    }
}

/// A completed speaker turn: the speech run that was closed by silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// Speech frames in the turn.
    pub frames: usize,
    /// Approximate duration in ms at the sensor's sample rate.
    pub duration_ms: u64,
    /// The raw i16 mono samples of the speech run (for the ASR backend).
    pub samples: Vec<i16>,
}

/// Pure turn detector: accumulates speech frames into a [`Turn`] and closes it
/// after `end_silence_frames` consecutive silent frames. `None` while the turn
/// is still open (or nothing is being said).
#[derive(Debug, Clone)]
pub struct TurnDetector {
    /// Consecutive silent frames that end an open turn.
    pub end_silence_frames: usize,
    /// Samples per frame (for the duration estimate).
    pub sample_rate: u32,
    in_speech: bool,
    speech_frames: usize,
    silence_frames: usize,
    samples: Vec<i16>,
}

impl Default for TurnDetector {
    fn default() -> Self {
        Self {
            end_silence_frames: 5,
            sample_rate: DEFAULT_SAMPLE_RATE,
            in_speech: false,
            speech_frames: 0,
            silence_frames: 0,
            samples: Vec::new(),
        }
    }
}

impl TurnDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sample_rate(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ..Self::default()
        }
    }

    /// Feed one classified frame. `speech` is the VAD verdict for `frame`.
    /// Returns the closed [`Turn`] when this frame ends one — callers then own
    /// the samples (they were moved out).
    pub fn feed(&mut self, speech: bool, frame: &[i16]) -> Option<Turn> {
        if speech {
            self.in_speech = true;
            self.speech_frames += 1;
            self.silence_frames = 0;
            self.samples.extend_from_slice(frame);
            return None;
        }
        if !self.in_speech {
            return None;
        }
        self.silence_frames += 1;
        if self.silence_frames < self.end_silence_frames {
            return None;
        }
        let duration_ms = (self.speech_frames as u64 * FRAME_SAMPLES as u64 * 1000)
            / u64::from(self.sample_rate.max(1));
        let turn = Turn {
            frames: self.speech_frames,
            duration_ms,
            samples: std::mem::take(&mut self.samples),
        };
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        Some(turn)
    }

    pub fn reset(&mut self) {
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.samples.clear();
    }
}

/// ASR seam — the frozen plug-in point for real speech-to-text backends.
/// A turn's samples are handed to the attached backend; the result rides on
/// the workspace event. Nothing in the pipeline hard-depends on a backend.
#[async_trait]
pub trait SpeechToText: Send + Sync {
    fn name(&self) -> &str;

    /// Transcribe a mono i16 turn. `sample_rate` is the sensor's rate.
    async fn transcribe(&self, samples: &[i16], sample_rate: u32) -> Result<String, String>;
}

/// Deterministic ASR backend for tests/CI: returns a fixed phrase.
pub struct MockTranscriber {
    pub phrase: String,
}

#[async_trait]
impl SpeechToText for MockTranscriber {
    fn name(&self) -> &str {
        "mock"
    }

    async fn transcribe(&self, _samples: &[i16], _sample_rate: u32) -> Result<String, String> {
        Ok(self.phrase.clone())
    }
}

/// Build the `sensor.audio.turn` workspace event. Pure. `transcript`/`asr` are
/// `Some` only when an ASR backend produced them — the payload is honest.
pub fn build_turn_event(
    turn: &Turn,
    sample_rate: u32,
    transcript: Option<(String, String)>, // (asr name, text)
) -> WorkspaceEvent {
    let (asr, text) = match transcript {
        Some((name, text)) => (Some(name), Some(text)),
        None => (None, None),
    };
    WorkspaceEvent::new(
        Topic::TurnDetected,
        serde_json::json!({
            "frames": turn.frames,
            "duration_ms": turn.duration_ms,
            "sample_rate": sample_rate,
            "asr": asr,
            "transcript": text,
        }),
    )
}

/// The real audio sensor: WASAPI mic capture → VAD → turn detection → optional
/// ASR → [`Topic::TurnDetected`] events. On Windows it captures the default
/// input device; on other platforms (or when no capture device exists) it
/// stays silent — it never fabricates turns. `process_pcm` is exposed so the
/// whole pipeline is testable end-to-end over synthetic frames everywhere.
pub struct AudioSensor {
    vad: Vad,
    turns: TurnDetector,
    asr: Option<Arc<dyn SpeechToText>>,
    capture: Option<WasapiCapture>,
    /// Partial frame left over between polls.
    pending: Vec<i16>,
}

impl Default for AudioSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSensor {
    pub fn new() -> Self {
        Self {
            vad: Vad::default(),
            turns: TurnDetector::with_sample_rate(DEFAULT_SAMPLE_RATE),
            asr: None,
            capture: open_default_capture(),
            pending: Vec::new(),
        }
    }

    /// Build a sensor with no capture device (headless/CI, or when only the
    /// pure pipeline is wanted). `poll` returns nothing; `process_pcm` still
    /// works, so tests can drive the full VAD → turn → event path.
    pub fn without_capture() -> Self {
        Self {
            vad: Vad::default(),
            turns: TurnDetector::with_sample_rate(DEFAULT_SAMPLE_RATE),
            asr: None,
            capture: None,
            pending: Vec::new(),
        }
    }

    /// Attach an ASR backend. Until then, turns carry `transcript: null`.
    pub fn with_asr(mut self, asr: Arc<dyn SpeechToText>) -> Self {
        self.asr = Some(asr);
        self
    }

    /// Whether a capture device is actually open (headless/CI = false).
    pub fn capture_available(&self) -> bool {
        self.capture.is_some()
    }

    /// Pure pipeline entry: feed raw i16 PCM and get back any closed turns.
    /// Used by `poll` on captured audio and by tests over synthetic audio.
    pub fn process_pcm(&mut self, samples: &[i16]) -> Vec<Turn> {
        self.pending.extend_from_slice(samples);
        let mut closed = Vec::new();
        while self.pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = self.pending.drain(..FRAME_SAMPLES).collect();
            let speech = self.vad.classify(rms(&frame));
            if let Some(turn) = self.turns.feed(speech, &frame) {
                closed.push(turn);
            }
        }
        closed
    }

    /// Turn closed turns into workspace events (running the ASR seam when
    /// attached). Separate from `process_pcm` so tests can inspect both halves.
    pub async fn turns_to_events(&self, turns: Vec<Turn>) -> Vec<WorkspaceEvent> {
        let mut events = Vec::new();
        for turn in turns {
            let transcript = match &self.asr {
                Some(asr) => {
                    match asr.transcribe(&turn.samples, self.turns.sample_rate).await {
                        Ok(text) => Some((asr.name().to_string(), text)),
                        Err(_) => None, // honest: no transcript
                    }
                }
                None => None,
            };
            events.push(build_turn_event(&turn, self.turns.sample_rate, transcript));
        }
        events
    }
}

#[async_trait]
impl Sensor for AudioSensor {
    fn name(&self) -> &str {
        "windows_mic"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.audio"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let Some(capture) = &self.capture else {
            return Vec::new();
        };
        let Some(samples) = capture.read_available() else {
            return Vec::new();
        };
        let turns = self.process_pcm(&samples);
        self.turns_to_events(turns).await
    }
}

// ── Real capture (Windows only) ───────────────────────────────────

/// A live WASAPI shared-mode capture stream. Wrapped in `Option` so a sensor
/// without a device (headless/CI) is quiet rather than broken.
///
/// `channels`/`bits_per_sample`/`is_float` describe the format the client
/// actually negotiated (our preferred 16 kHz mono PCM, or the engine's mix
/// format as a fallback); `read_available` converts every packet to i16 mono
/// so the VAD sees exactly one shape regardless of hardware.
#[cfg(target_os = "windows")]
struct WasapiCapture {
    #[allow(dead_code)] // held alive for the stream lifetime
    client: windows::Win32::Media::Audio::IAudioClient,
    capture: windows::Win32::Media::Audio::IAudioCaptureClient,
    channels: u16,
    bits_per_sample: u16,
    is_float: bool,
}

// Never constructed on non-Windows; keeps `AudioSensor`'s `Option<WasapiCapture>`
// field well-formed on every target.
#[cfg(not(target_os = "windows"))]
struct WasapiCapture {}

// The COM interfaces are not `Send`/`Sync` by default in windows-rs. They are
// created under MTA (`CoInitializeEx(COINIT_MULTITHREADED)`), which makes them
// callable from any thread, and only ever touched from `AudioSensor::poll` —
// never handed across threads. `&AudioSensor` is held across the ASR await in
// `turns_to_events`, so both bounds are required; both are sound here.
#[cfg(target_os = "windows")]
unsafe impl Send for WasapiCapture {}
#[cfg(target_os = "windows")]
unsafe impl Sync for WasapiCapture {}

/// Open the default input device in shared mode. First try our preferred
/// 16 kHz mono 16-bit PCM (the VAD's native shape — no resampling); if the
/// engine rejects it, fall back to `GetMixFormat` and convert packets to i16
/// mono in `read_available`. `None` = no device / permission / COM failure —
/// an honest silence.
#[cfg(target_os = "windows")]
fn open_default_capture() -> Option<WasapiCapture> {
    use windows::core::GUID;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        // MTA: the COM interfaces may be called from any tokio worker thread.
        // `CoInitializeEx` returns a raw HRESULT (not a Result); S_FALSE (already
        // initialized) is success — `is_err` covers the real failures.
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return None;
        }

        // CLSID_MMDeviceEnumerator — the standard Windows audio endpoint enumerator.
        let mmdevice_enumerator = GUID::from_u128(0xbcde0395e52f467c_8e3dc4579291692e);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&mmdevice_enumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .ok()?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).ok()?;

        // Preferred format: 16 kHz mono 16-bit PCM. Shared mode accepts a
        // client-side format different from the mix format — the engine
        // converts — but not every stack does, so we fall back below.
        let preferred = WAVEFORMATEX {
            wFormatTag: 1, // WAVE_FORMAT_PCM
            nChannels: 1,
            nSamplesPerSec: DEFAULT_SAMPLE_RATE,
            nAvgBytesPerSec: DEFAULT_SAMPLE_RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // 1 second buffer (hns = 100 ns units), shared mode, no event flag.
        if client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 10_000_000, 0, &preferred, None)
            .is_ok()
        {
            let capture: IAudioCaptureClient = client.GetService().ok()?;
            client.Start().ok()?;
            return Some(WasapiCapture {
                client,
                capture,
                channels: 1,
                bits_per_sample: 16,
                is_float: false,
            });
        }

        // Fallback: the engine's own mix format (often float32 / N channels).
        // We only accept PCM or IEEE-float at 16 or 32 bits — anything else is
        // left to the honest-silence rule rather than guessing. For the
        // EXTENSIBLE tag the real format lives in the SubFormat GUID.
        // (WAVE_FORMAT_IEEE_FLOAT = 3, WAVE_FORMAT_EXTENSIBLE = 0xFFFE.)
        let mix = client.GetMixFormat().ok()?;
        if (*mix).wBitsPerSample != 16 && (*mix).wBitsPerSample != 32 {
            return None;
        }
        let float_sub = GUID::from_u128(0x0000000300000010_800000aa00389b71);
        let pcm_sub = GUID::from_u128(0x0000000100000010_800000aa00389b71);
        let (is_float, pcm_ok) = match (*mix).wFormatTag {
            1 => (false, true), // WAVE_FORMAT_PCM
            3 => (true, true),  // WAVE_FORMAT_IEEE_FLOAT
            0xFFFE => {
                // KSDATAFORMAT_SUBTYPE_PCM / _IEEE_FLOAT inside the extension.
                let sub =
                    (&*mix.cast::<windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE>()).SubFormat;
                if sub == float_sub {
                    (true, true)
                } else if sub == pcm_sub {
                    (false, true)
                } else {
                    (false, false)
                }
            }
            _ => (false, false),
        };
        if !pcm_ok {
            return None;
        }
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 10_000_000, 0, mix, None)
            .ok()?;
        let capture: IAudioCaptureClient = client.GetService().ok()?;
        client.Start().ok()?;

        Some(WasapiCapture {
            client,
            capture,
            channels: (*mix).nChannels.max(1),
            bits_per_sample: (*mix).wBitsPerSample,
            is_float,
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn open_default_capture() -> Option<WasapiCapture> {
    None
}

#[cfg(target_os = "windows")]
impl WasapiCapture {
    /// Drain every pending packet into one i16 mono buffer (converting the
    /// negotiated format — mono, stereo/N-channel, PCM or float). `None` on
    /// error (stream torn down) — the sensor goes quiet rather than panicking.
    fn read_available(&self) -> Option<Vec<i16>> {
        use windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT;

        unsafe {
            let mut out = Vec::new();
            loop {
                let packet = self.capture.GetNextPacketSize().ok()?;
                if packet == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames: u32 = 0;
                let mut flags: u32 = 0;
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .ok()?;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    // Silent packet: the buffer may be garbage — emit zeros.
                    out.resize(out.len() + frames as usize, 0);
                } else if self.bits_per_sample == 16 {
                    let pcm = std::slice::from_raw_parts(
                        data as *const i16,
                        frames as usize * self.channels as usize,
                    );
                    out.extend(pcm.iter().step_by(self.channels as usize).copied());
                } else if self.is_float {
                    // 32-bit float: clamp + scale to i16.
                    let f32s = std::slice::from_raw_parts(
                        data as *const f32,
                        frames as usize * self.channels as usize,
                    );
                    for sample in f32s.iter().step_by(self.channels as usize) {
                        let v = sample.clamp(-1.0, 1.0);
                        out.push((v * i16::MAX as f32) as i16);
                    }
                } else {
                    // 32-bit integer PCM: take the top 16 bits.
                    let i32s = std::slice::from_raw_parts(
                        data as *const i32,
                        frames as usize * self.channels as usize,
                    );
                    for sample in i32s.iter().step_by(self.channels as usize) {
                        out.push((sample >> 16) as i16);
                    }
                }
                self.capture.ReleaseBuffer(frames).ok()?;
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine tone at `freq` Hz — deterministic "speech-like" energy.
    fn tone(frames: usize, freq: f64, sample_rate: u32) -> Vec<i16> {
        let mut out = Vec::with_capacity(frames * FRAME_SAMPLES);
        for i in 0..frames * FRAME_SAMPLES {
            let t = i as f64 / f64::from(sample_rate);
            let v = 0.3 * (2.0 * std::f64::consts::PI * freq * t).sin();
            out.push((v * i16::MAX as f64) as i16);
        }
        out
    }

    fn silence(frames: usize) -> Vec<i16> {
        vec![0i16; frames * FRAME_SAMPLES]
    }

    #[test]
    fn rms_zero_for_silence_and_positive_for_tone() {
        assert_eq!(rms(&[0i16; 64]), 0.0);
        assert!(rms(&tone(1, 440.0, DEFAULT_SAMPLE_RATE)) > 0.05);
        assert!(rms(&[i16::MAX; 16]) > 0.9);
    }

    #[test]
    fn vad_classifies_speech_and_silence_with_hangover() {
        let mut vad = Vad::new();
        assert!(vad.classify(0.1)); // loud → speech
        assert!(vad.classify(0.0)); // quiet #1 inside hangover → still speech
        assert!(vad.classify(0.0)); // quiet #2 (hangover_frames=2) → still speech
        assert!(!vad.classify(0.0)); // quiet #3 → flips to silence
        assert!(!vad.classify(0.0)); // stays silent
        assert!(vad.classify(0.5)); // loud again
    }

    #[test]
    fn silence_never_starts_a_turn() {
        let mut detector = TurnDetector::new();
        for _ in 0..20 {
            assert!(detector.feed(false, &[0i16; FRAME_SAMPLES]).is_none());
        }
    }

    #[test]
    fn speech_run_closed_by_silence_yields_a_turn() {
        let mut detector = TurnDetector::new();
        let mut turn = None;
        for _ in 0..10 {
            turn = detector.feed(true, &[1i16; FRAME_SAMPLES]);
        }
        assert!(turn.is_none(), "still open while speaking");
        for _ in 0..5 {
            turn = detector.feed(false, &[0i16; FRAME_SAMPLES]);
        }
        let turn = turn.expect("turn closes after end_silence_frames");
        assert_eq!(turn.frames, 10);
        assert_eq!(turn.samples.len(), 10 * FRAME_SAMPLES);
        assert_eq!(turn.duration_ms, 200); // 10 × 20 ms
                                           // After the close, more silence yields nothing new.
        assert!(detector.feed(false, &[0i16; FRAME_SAMPLES]).is_none());
    }

    #[test]
    fn brief_pause_does_not_split_the_turn() {
        let mut detector = TurnDetector::new();
        let mut last = None;
        for f in 0..20 {
            let speech = !(f == 8 || f == 9); // 2-frame pause < end_silence_frames
            last = detector.feed(speech, &[if speech { 1 } else { 0 }; FRAME_SAMPLES]);
        }
        assert!(last.is_none(), "pause inside hangover keeps turn open");
    }

    #[test]
    fn event_payload_is_honest_without_asr() {
        let turn = Turn {
            frames: 10,
            duration_ms: 200,
            samples: vec![1i16; 10 * FRAME_SAMPLES],
        };
        let event = build_turn_event(&turn, DEFAULT_SAMPLE_RATE, None);
        assert_eq!(event.topic, Topic::TurnDetected);
        assert!(event.payload["transcript"].is_null());
        assert!(event.payload["asr"].is_null());
        assert_eq!(event.payload["duration_ms"], 200);
        assert_eq!(event.payload["frames"], 10);
        assert_eq!(event.payload["sample_rate"], 16_000);
    }

    #[test]
    fn event_payload_carries_mock_transcript() {
        let turn = Turn {
            frames: 5,
            duration_ms: 100,
            samples: vec![1i16; 5 * FRAME_SAMPLES],
        };
        let event = build_turn_event(
            &turn,
            DEFAULT_SAMPLE_RATE,
            Some(("mock".into(), "hello".into())),
        );
        assert_eq!(event.payload["transcript"], "hello");
        assert_eq!(event.payload["asr"], "mock");
    }

    #[tokio::test]
    async fn audio_sensor_events_flow_through_the_sensor_loop() {
        use crate::SensorLoop;

        // Drive the sensor's pure pipeline first (no mic needed), then poll a
        // loop that owns it — the resulting TurnDetected must land in the
        // workspace with the correct topic.
        let mut sensor = AudioSensor::without_capture();
        let mut pcm = tone(10, 440.0, DEFAULT_SAMPLE_RATE);
        pcm.extend(silence(8));
        let turns = sensor.process_pcm(&pcm);
        let events = sensor.turns_to_events(turns).await;
        assert_eq!(events.len(), 1);

        // The events the pipeline produced ride through the SensorLoop exactly
        // like any captured batch: published under sensor.audio.
        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(crate::MockSensor::new("audio_events", events)));
        let workspace = friday_cognitive::workspace::GlobalWorkspace::new();
        let topics = loop_.poll_all_with_topics(&workspace).await;
        assert_eq!(topics, vec!["sensor.audio.turn"]);
        assert_eq!(workspace.history(None).len(), 1);
        assert_eq!(workspace.history(None)[0].topic, Topic::TurnDetected);
    }

    #[tokio::test]
    async fn end_to_end_synthetic_turn_publishes_event() {
        let asr: Arc<dyn SpeechToText> = Arc::new(MockTranscriber {
            phrase: "synthetic hello".into(),
        });
        let mut sensor = AudioSensor::without_capture().with_asr(asr);
        // Tone + trailing silence in one feed: VAD sees speech (2 silent frames
        // ride the hangover), the turn detector closes it after 5 true-silent
        // frames. 8 silent frames guarantees the close.
        let mut pcm = tone(10, 440.0, DEFAULT_SAMPLE_RATE);
        pcm.extend(silence(8));
        let turns = sensor.process_pcm(&pcm);
        assert_eq!(turns.len(), 1);
        let events = sensor.turns_to_events(turns).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, Topic::TurnDetected);
        assert_eq!(events[0].payload["transcript"], "synthetic hello");
        assert_eq!(events[0].payload["asr"], "mock");
    }

    #[tokio::test]
    async fn end_to_end_synthetic_turn_without_asr_is_null_transcript() {
        let mut sensor = AudioSensor::without_capture(); // no ASR attached
        let mut pcm = tone(10, 440.0, DEFAULT_SAMPLE_RATE);
        pcm.extend(silence(8));
        let turns = sensor.process_pcm(&pcm);
        assert_eq!(turns.len(), 1);
        let events = sensor.turns_to_events(turns).await;
        assert_eq!(events.len(), 1);
        assert!(events[0].payload["transcript"].is_null());
        assert!(events[0].payload["asr"].is_null());
    }

    #[test]
    fn pure_pipeline_ignores_leading_silence() {
        let mut sensor = AudioSensor::without_capture();
        let mut pcm = silence(4);
        pcm.extend(tone(3, 440.0, DEFAULT_SAMPLE_RATE));
        pcm.extend(silence(8));
        let turns = sensor.process_pcm(&pcm);
        assert_eq!(turns.len(), 1);
        // 3 speech frames + 2 hangover frames (the first two silent frames
        // still count as speech) — the leading silence never opens a turn.
        assert_eq!(turns[0].frames, 5);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn real_mic_poll_is_tolerant_and_well_formed() {
        // Headless/no-device CI yields zero events; a real desktop yields a
        // well-formed TurnDetected only when someone actually spoke. Either
        // way: never panic, always the right topic when an event fires.
        let mut sensor = AudioSensor::new();
        let events = sensor.poll().await;
        for event in &events {
            assert_eq!(event.topic, Topic::TurnDetected);
            let p = &event.payload;
            assert!(p["duration_ms"].is_u64());
            assert!(p["frames"].is_u64());
            assert!(p["asr"].is_null() || p["asr"].is_string());
            assert!(p["transcript"].is_null() || p["transcript"].is_string());
        }
    }
}
