//! S8-B: whisper.cpp behind the [`SpeechToText`] seam.
//!
//! [`WhisperCppTranscriber`] runs the benchmarked whisper.cpp CLI
//! (`whisper-cli`) over a turn's 16 kHz mono PCM. Everything about the backend
//! is **explicit deployment configuration** — binary path, model, language,
//! threads and any extra runtime flags — nothing is hardcoded to a machine or
//! a benchmark directory. CPU vs GPU is a property of the configured
//! `executable` (point it at the CUDA build to run on the GPU, or at a CPU
//! build to stay off it); the adapter is identical either way.
//!
//! Honesty: a non-16 kHz sample rate, a missing binary, a non-zero exit, or a
//! run that produces no transcript all surface as `Err`, which the sensor's
//! pipeline already converts into a `transcript: null` turn — a failed
//! transcription is never invented into words.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use crate::audio::SpeechToText;

/// whisper.cpp runs at 16 kHz mono; the adapter refuses anything else rather
/// than silently mis-transcribing (the sensor always emits this rate anyway).
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Unique-per-process counter for temp WAV names.
pub(crate) static WAV_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Explicit backend configuration — the deployment contract for whisper.cpp.
///
/// CPU vs GPU is a property of `executable`: point it at the CUDA build's
/// `whisper-cli` to use the GPU, or at a CPU build to stay off it. Nothing here
/// assumes a benchmark directory or a specific machine.
#[derive(Debug, Clone)]
pub struct WhisperCppConfig {
    /// Path to the `whisper-cli` executable.
    pub executable: PathBuf,
    /// Path to a GGML model (e.g. `ggml-base.bin`).
    pub model: PathBuf,
    /// Spoken language for `-l` (whisper code, e.g. "en").
    pub language: String,
    /// Worker threads for `-t`.
    pub threads: u32,
    /// Extra runtime flags passed verbatim after the standard ones
    /// (`-np`/`-nt` are always set).
    pub extra_args: Vec<String>,
    /// Directory for the per-turn temp WAV (defaults to the OS temp dir).
    /// The real host should point this at a drive with headroom — the
    /// workspace drive is not it.
    pub temp_dir: PathBuf,
}

impl WhisperCppConfig {
    /// `language` defaults to `en`, `threads` to 4, temp files to the OS temp
    /// dir — every field is still overridable before building the transcriber.
    pub fn new(executable: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            model: model.into(),
            language: "en".into(),
            threads: 4,
            extra_args: Vec::new(),
            temp_dir: std::env::temp_dir(),
        }
    }
}

/// whisper.cpp prints one of these when it detects no speech in the segment.
/// A transcript that is only a marker is silence, not a user request.
fn is_blank_marker(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "[blank_audio]" | "[silence]" | "[no_speech]" | "[music]"
    )
}

/// Whisper.cpp wrapped in the frozen ASR seam.
pub struct WhisperCppTranscriber {
    config: WhisperCppConfig,
}

impl WhisperCppTranscriber {
    pub fn new(config: WhisperCppConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &WhisperCppConfig {
        &self.config
    }

    /// Assemble the CLI invocation for a given WAV path. Exposed so tests pin
    /// the exact command instead of guessing it.
    pub(crate) fn build_command(&self, wav: &Path) -> Command {
        let mut cmd = Command::new(&self.config.executable);
        cmd.arg("-m")
            .arg(&self.config.model)
            .arg("-f")
            .arg(wav)
            .arg("-l")
            .arg(&self.config.language)
            .arg("-t")
            .arg(self.config.threads.to_string())
            .arg("-np")
            .arg("-nt");
        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }
        cmd
    }

    /// Turn whisper-cli's captured stdout into a transcript: the non-empty,
    /// trimmed lines joined with spaces. `-np -nt` keep stdout to just the
    /// transcript text. `None` = nothing was recognized — including whisper's
    /// own "no speech" markers, which must count as silence rather than being
    /// published as fabricated goals.
    pub(crate) fn parse_transcript(&self, stdout: &str) -> Option<String> {
        let lines: Vec<&str> = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !is_blank_marker(l))
            .collect();
        if lines.is_empty() {
            None
        } else {
            Some(lines.join(" "))
        }
    }

    /// Write the turn as a 16 kHz mono 16-bit WAV in `temp_dir`, named
    /// uniquely per call so a transcription never collides with a prior one.
    fn write_wav(&self, samples: &[i16]) -> Result<PathBuf, String> {
        let path = self.config.temp_dir.join(format!(
            "friday_whisper_{}_{}.wav",
            std::process::id(),
            WAV_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        write_wav(&path, samples, WHISPER_SAMPLE_RATE)
            .map_err(|e| format!("write temp wav: {e}"))?;
        Ok(path)
    }
}

#[async_trait]
impl SpeechToText for WhisperCppTranscriber {
    fn name(&self) -> &str {
        "whisper-cpp"
    }

    async fn transcribe(&self, samples: &[i16], sample_rate: u32) -> Result<String, String> {
        if sample_rate != WHISPER_SAMPLE_RATE {
            return Err(format!(
                "whisper.cpp expects {WHISPER_SAMPLE_RATE} Hz mono PCM, got {sample_rate} Hz"
            ));
        }
        if samples.is_empty() {
            return Err("empty turn: nothing to transcribe".into());
        }

        let wav = self.write_wav(samples)?;
        let mut command = self.build_command(&wav);

        // The CLI blocks for the whole utterance (~0.65 s warm on the
        // benchmarked GPU base build); never hold the async executor on it.
        let output = tokio::task::spawn_blocking(move || command.output())
            .await
            .map_err(|e| format!("transcribe task failed: {e}"))?
            .map_err(|e| format!("spawn whisper-cli: {e}"))?;

        let _ = std::fs::remove_file(&wav);

        if !output.status.success() {
            return Err(format!(
                "whisper.cpp exited with {}",
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "no code".into())
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_transcript(&stdout)
            .ok_or_else(|| "whisper.cpp produced no transcript".into())
    }
}

/// Minimal RIFF/WAVE writer for 16-bit PCM (mono): 44-byte header + samples.
/// Kept local (no dependency) because the only thing the backend needs is a
/// file whisper-cli can open.
pub(crate) fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioSensor, DEFAULT_SAMPLE_RATE, FRAME_SAMPLES};
    use friday_cognitive::workspace::Topic;

    /// Deterministic "speech-like" energy: 10 frames of 440 Hz tone + 8 silent
    /// frames, matching the sensor's own test pipeline so VAD opens and closes
    /// a turn.
    fn turn_pcm() -> Vec<i16> {
        let mut pcm: Vec<i16> = (0..10 * FRAME_SAMPLES)
            .map(|i| {
                let t = i as f64 / f64::from(DEFAULT_SAMPLE_RATE);
                (0.3 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() * i16::MAX as f64) as i16
            })
            .collect();
        pcm.extend_from_slice(&vec![0i16; 8 * FRAME_SAMPLES]);
        pcm
    }

    fn test_config(executable: PathBuf) -> WhisperCppConfig {
        WhisperCppConfig::new(executable, PathBuf::from("C:\\models\\ggml-base.bin"))
    }

    fn unique_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("friday_s8b_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn config_defaults_are_explicit_and_sane() {
        let cfg = WhisperCppConfig::new("whisper-cli", "model.bin");
        assert_eq!(cfg.language, "en");
        assert_eq!(cfg.threads, 4);
        assert!(cfg.extra_args.is_empty());
        assert_eq!(cfg.temp_dir, std::env::temp_dir());
        assert_eq!(cfg.executable, PathBuf::from("whisper-cli"));
    }

    #[test]
    fn write_wav_produces_16k_mono_16bit_round_trippable_file() {
        let dir = unique_dir("wav");
        let path = dir.join("tone.wav");
        let pcm = turn_pcm();
        write_wav(&path, &pcm, WHISPER_SAMPLE_RATE).expect("write wav");

        let bytes = std::fs::read(&path).expect("read wav back");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            16_000
        );
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16); // bits

        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data_len as usize, pcm.len() * 2);
        let mut read_back = Vec::with_capacity(pcm.len());
        for chunk in bytes[44..].chunks_exact(2) {
            read_back.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        assert_eq!(read_back, pcm, "PCM must round-trip bit-for-bit");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_command_pins_the_invocation() {
        let mut cfg = test_config(PathBuf::from("bin\\whisper-cli.exe"));
        cfg.model = PathBuf::from("ggml-base.bin");
        cfg.language = "en".into();
        cfg.threads = 4;
        cfg.extra_args = vec!["-pp".into(), "--no-gpu".into()];
        let t = WhisperCppTranscriber::new(cfg);
        let cmd = t.build_command(Path::new("C:\\tmp\\turn.wav"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "-m",
                "ggml-base.bin",
                "-f",
                "C:\\tmp\\turn.wav",
                "-l",
                "en",
                "-t",
                "4",
                "-np",
                "-nt",
                "-pp",
                "--no-gpu",
            ]
        );
    }

    #[test]
    fn parse_transcript_joins_non_empty_trimmed_lines() {
        let t = WhisperCppTranscriber::new(test_config("whisper-cli".into()));
        assert_eq!(
            t.parse_transcript("  open Chrome \r\n"),
            Some("open Chrome".into())
        );
        assert_eq!(
            t.parse_transcript("hello\n world\n"),
            Some("hello world".into())
        );
        assert_eq!(t.parse_transcript("  \n\t\n"), None);
        assert_eq!(t.parse_transcript(""), None);
    }

    #[test]
    fn parse_transcript_treats_whisper_silence_markers_as_no_transcript() {
        let t = WhisperCppTranscriber::new(test_config("whisper-cli".into()));
        // The exact marker whisper.cpp emitted during S8-C live: quiet room
        // audio must not become a fabricated goal in plan.goal.
        assert_eq!(t.parse_transcript("[BLANK_AUDIO]\n"), None);
        assert_eq!(t.parse_transcript("[silence]"), None);
        assert_eq!(t.parse_transcript("[MUSIC]"), None);
        // Real words around markers still pass through as a real transcript.
        assert_eq!(
            t.parse_transcript("open Chrome\n[silence]"),
            Some("open Chrome".into())
        );
    }

    #[tokio::test]
    async fn transcribe_refuses_non_16k_sample_rate_honestly() {
        let t = WhisperCppTranscriber::new(test_config("whisper-cli".into()));
        let err = t.transcribe(&turn_pcm(), 44_100).await.unwrap_err();
        assert!(err.contains("16000"), "err must name the contract: {err}");
    }

    #[tokio::test]
    async fn transcribe_refuses_an_empty_turn() {
        let t = WhisperCppTranscriber::new(test_config("whisper-cli".into()));
        let err = t.transcribe(&[], DEFAULT_SAMPLE_RATE).await.unwrap_err();
        assert!(err.contains("empty"), "err must say why: {err}");
    }

    /// A fake `whisper-cli` as a batch script: ignores args, echoes a fixed
    /// transcript, exits 0. Drives the spawn → capture → parse path hermetically.
    fn mock_binary(dir: &Path, body: &str) -> PathBuf {
        let bat = dir.join("mock-whisper-cli.bat");
        std::fs::write(&bat, body).expect("write mock binary");
        bat
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn mock_backend_yields_a_transcript_through_the_adapter() {
        let dir = unique_dir("ok");
        let bat = mock_binary(&dir, "@echo off\r\necho open Chrome\r\nexit /b 0\r\n");
        let mut cfg = test_config(bat);
        cfg.temp_dir = dir.clone();
        let t = WhisperCppTranscriber::new(cfg);

        let text = t.transcribe(&turn_pcm(), DEFAULT_SAMPLE_RATE).await;
        assert_eq!(text, Ok("open Chrome".into()));
        // The temp WAV was cleaned up.
        assert!(
            !std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("friday_whisper_")),
            "temp wav must be removed after the run"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn mock_backend_failure_is_reported_honestly() {
        let dir = unique_dir("fail");
        let bat = mock_binary(&dir, "@echo off\r\nexit /b 1\r\n");
        let mut cfg = test_config(bat);
        cfg.temp_dir = dir.clone();
        let t = WhisperCppTranscriber::new(cfg);

        let err = t
            .transcribe(&turn_pcm(), DEFAULT_SAMPLE_RATE)
            .await
            .unwrap_err();
        assert!(
            err.contains("exited with 1"),
            "err must report the exit: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn mock_backend_with_silence_is_an_err_not_words() {
        let dir = unique_dir("silence");
        let bat = mock_binary(&dir, "@echo off\r\nrem nothing on stdout\r\nexit /b 0\r\n");
        let mut cfg = test_config(bat);
        cfg.temp_dir = dir.clone();
        let t = WhisperCppTranscriber::new(cfg);

        let err = t
            .transcribe(&turn_pcm(), DEFAULT_SAMPLE_RATE)
            .await
            .unwrap_err();
        assert!(err.contains("no transcript"), "err must say why: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The S8-B trait-boundary test: the transcriber rides the existing
    /// AudioSensor pipeline end to end — VAD → turn → ASR → TurnDetected with
    /// the transcript in the payload, exactly the event VoiceIntake consumes.
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn audio_sensor_with_whisper_backend_emits_transcripted_turns() {
        let dir = unique_dir("boundary");
        let bat = mock_binary(&dir, "@echo off\r\necho open Chrome\r\nexit /b 0\r\n");
        let mut cfg = test_config(bat);
        cfg.temp_dir = dir.clone();
        let asr = std::sync::Arc::new(WhisperCppTranscriber::new(cfg));

        let mut sensor = AudioSensor::without_capture().with_asr(asr);
        let turns = sensor.process_pcm(&turn_pcm());
        assert_eq!(turns.len(), 1, "a turn must close over the tone + silence");
        let events = sensor.turns_to_events(turns).await;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, Topic::TurnDetected);
        assert_eq!(events[0].payload["transcript"], "open Chrome");
        assert_eq!(events[0].payload["asr"], "whisper-cpp");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Read a 16 kHz mono 16-bit WAV back into i16 samples (for the live test).
    /// Scans RIFF chunks (the fmt chunk can be 16 or 18 bytes + optional extra
    /// chunks between fmt and data, so nothing is assumed at a fixed offset).
    fn read_wav_pcm(path: &Path) -> Vec<i16> {
        let bytes = std::fs::read(path).expect("read wav");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let mut fmt = None;
        let mut data = None;
        let mut offset = 12;
        while offset + 8 <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let body = &bytes[offset + 8..offset + 8 + size];
            if id == b"fmt " {
                fmt = Some((
                    u16::from_le_bytes(body[0..2].try_into().unwrap()), // format
                    u16::from_le_bytes(body[2..4].try_into().unwrap()), // channels
                    u32::from_le_bytes(body[4..8].try_into().unwrap()), // rate
                    u16::from_le_bytes(body[14..16].try_into().unwrap()), // bits
                ));
            } else if id == b"data" {
                data = Some(body);
            }
            offset += 8 + size + (size % 2); // chunks are word-aligned
        }
        let (format, channels, rate, bits) = fmt.expect("fmt chunk");
        assert_eq!(format, 1, "PCM");
        assert_eq!(channels, 1, "mono");
        assert_eq!(rate, 16_000);
        assert_eq!(bits, 16, "16-bit");
        let body = data.expect("data chunk");
        body.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    /// Live-backend verification — NOT part of the normal suite. Point
    /// `FRIDAY_WHISPER_EXE` / `FRIDAY_WHISPER_MODEL` / `FRIDAY_WHISPER_WAV` at a
    /// whisper.cpp install + a 16 kHz mono speech file, then run with
    /// `--ignored`. This is the deployment-config boundary: no machine-specific
    /// path lives in code. Skipped silently when the env is unset.
    #[ignore]
    #[tokio::test]
    async fn live_whisper_backend_transcribes_real_speech() {
        let (exe, model, wav) = (
            std::env::var("FRIDAY_WHISPER_EXE").ok(),
            std::env::var("FRIDAY_WHISPER_MODEL").ok(),
            std::env::var("FRIDAY_WHISPER_WAV").ok(),
        );
        let (Some(exe), Some(model), Some(wav)) = (exe, model, wav) else {
            eprintln!("SKIP: set FRIDAY_WHISPER_EXE/MODEL/WAV for the live backend check");
            return;
        };
        let pcm = read_wav_pcm(Path::new(&wav));
        let t = WhisperCppTranscriber::new(WhisperCppConfig::new(exe, model));
        let text = t.transcribe(&pcm, DEFAULT_SAMPLE_RATE).await;
        assert!(
            matches!(&text, Ok(s) if !s.trim().is_empty()),
            "real whisper-cpp must transcribe real speech, got: {text:?}"
        );
        eprintln!("live whisper-cpp transcript: {:?}", text);
    }
}
