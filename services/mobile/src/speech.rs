//! FRIDAY's voice out: answers spoken on this computer, nothing sent anywhere.
//!
//! A small neural voice (Supertonic or Kitten, run by the pinned sherpa-onnx
//! engine on the CPU) turns text into speech one sentence at a time, so the
//! first sentence is heard while the next is still being made. Everything that
//! decides anything is pure and tested without audio: what is worth saying
//! (`clean_for_speech`), where to break it (`sentences`), how loud it comes out
//! (`normalize`), and the order of work (`speak` with a fake engine and
//! player). The engine and the player are two thin process calls.
//!
//! Nothing here plays sound unless `say` is asked to; tests never do.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

/// Which model is speaking. Supertonic is recognised by its files; the others
/// are named in `settings.json`. TinyVoca runs in this process (the sherpa-onnx
/// engine package only lends it ONNX Runtime); the rest run the engine program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Supertonic,
    Kitten,
    Kokoro,
    TinyVoca,
}

/// Kokoro's English female voices as (name, speaker id in the model). The
/// person's `speaker` setting is the position in this list, so 0 (the default)
/// is Bella.
pub const KOKORO_VOICES: [(&str, u32); 8] = [
    ("Bella", 2),
    ("Heart", 3),
    ("Nicole", 6),
    ("Sarah", 9),
    ("Sky", 10),
    ("Nova", 7),
    ("Emma", 21),
    ("Isabella", 22),
];

/// The names of the voices `kind` offers, in the order the `speaker` setting counts them.
pub fn voice_names(kind: Kind) -> Vec<String> {
    match kind {
        Kind::Kokoro => KOKORO_VOICES.iter().map(|(n, _)| (*n).into()).collect(),
        Kind::Kitten => [
            "Bella", "Jasper", "Luna", "Bruno", "Rosie", "Hugo", "Kiki", "Leo",
        ]
        .map(String::from)
        .into(),
        Kind::Supertonic => (1..=10).map(|n| format!("Voice {n}")).collect(),
        Kind::TinyVoca => crate::tinyvoca::VOICES.map(String::from).into(),
    }
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "supertonic" => Some(Kind::Supertonic),
            "kitten" => Some(Kind::Kitten),
            "kokoro" => Some(Kind::Kokoro),
            "tinyvoca" | "tiny-voca" => Some(Kind::TinyVoca),
            _ => None,
        }
    }
}

/// A voice that can speak: the engine, its model and the person's choices.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub engine: PathBuf,
    pub model: PathBuf,
    pub kind: Kind,
    /// The speaker (a Kitten name's number, a Supertonic style) in the model.
    pub speaker: u32,
    /// 1.0 is the model's own pace.
    pub speed: f32,
    pub threads: u32,
}

/// Where the voice lives: `FRIDAY_VOICE_DIR`, else `friday/voice` in the user's data folder.
pub fn voice_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FRIDAY_VOICE_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("friday").join("voice")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("friday").join("voice")
    }
}

fn engine_name() -> &'static str {
    if cfg!(windows) {
        "sherpa-onnx-offline-tts.exe"
    } else {
        "sherpa-onnx-offline-tts"
    }
}

/// The voice installed in `dir`, if there is one: `dir/bin/<engine>`, the model in
/// `dir/model`, and `dir/settings.json` naming its kind (and, optionally, the
/// speaker, speed and threads). A model that looks like Supertonic's is
/// recognised without a settings file.
pub fn detect(dir: &Path) -> Option<Voice> {
    let engine = dir.join("bin").join(engine_name());
    let model = dir.join("model");
    if !engine.is_file() || !model.is_dir() {
        return None;
    }
    let settings: Value = std::fs::read_to_string(dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let kind = settings["kind"]
        .as_str()
        .and_then(Kind::parse)
        .or_else(|| {
            model
                .join("duration_predictor.int8.onnx")
                .is_file()
                .then_some(Kind::Supertonic)
        })?;
    Some(Voice {
        engine,
        model,
        kind,
        speaker: settings["speaker"].as_u64().unwrap_or(0).min(63) as u32,
        speed: settings["speed"].as_f64().unwrap_or(1.0).clamp(0.6, 1.6) as f32,
        threads: settings["threads"].as_u64().unwrap_or(2).clamp(1, 8) as u32,
    })
}

/// The engine's command line for one piece of text.
pub fn engine_args(v: &Voice, text: &str, out: &Path) -> Vec<String> {
    let m = |f: &str| v.model.join(f).to_string_lossy().into_owned();
    // Kokoro's speaker setting is a position in its list of voices.
    let sid = match v.kind {
        Kind::Kokoro => KOKORO_VOICES[(v.speaker as usize).min(KOKORO_VOICES.len() - 1)].1,
        _ => v.speaker,
    };
    let mut a = vec![
        format!("--num-threads={}", v.threads),
        format!("--sid={sid}"),
        format!("--output-filename={}", out.display()),
    ];
    match v.kind {
        Kind::Supertonic => a.extend([
            format!(
                "--supertonic-duration-predictor={}",
                m("duration_predictor.int8.onnx")
            ),
            format!("--supertonic-text-encoder={}", m("text_encoder.int8.onnx")),
            format!(
                "--supertonic-vector-estimator={}",
                m("vector_estimator.int8.onnx")
            ),
            format!("--supertonic-vocoder={}", m("vocoder.int8.onnx")),
            format!("--supertonic-tts-json={}", m("tts.json")),
            format!("--supertonic-unicode-indexer={}", m("unicode_indexer.bin")),
            format!("--supertonic-voice-style={}", m("voice.bin")),
            "--lang=en".into(),
            "--num-steps=8".into(),
            format!("--speed={}", v.speed),
        ]),
        Kind::Kitten => a.extend([
            format!("--kitten-model={}", m("model.int8.onnx")),
            format!("--kitten-voices={}", m("voices.bin")),
            format!("--kitten-tokens={}", m("tokens.txt")),
            format!("--kitten-data-dir={}", m("espeak-ng-data")),
            format!("--kitten-length-scale={}", 1.0 / v.speed),
        ]),
        Kind::Kokoro => a.extend([
            format!("--kokoro-model={}", m("model.int8.onnx")),
            format!("--kokoro-voices={}", m("voices.bin")),
            format!("--kokoro-tokens={}", m("tokens.txt")),
            format!("--kokoro-data-dir={}", m("espeak-ng-data")),
            // English only: the Chinese lexicon and dictionary are not loaded.
            format!("--kokoro-lexicon={}", m("lexicon-us-en.txt")),
            format!("--kokoro-length-scale={}", 1.0 / v.speed),
        ]),
        // Spoken in this process: there is no command line.
        Kind::TinyVoca => {}
    }
    a.push(text.to_string());
    a
}

// ---------------------------------------------------------------- text

/// What is worth saying aloud from an answer written for the eye: code blocks
/// and bare links are skipped or named, markdown marks and emoji dropped,
/// symbols spoken as words.
pub fn clean_for_speech(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            if in_code {
                in_code = false;
            } else {
                in_code = true;
                out.push_str("There is some code on screen. ");
            }
            continue;
        }
        if in_code || line.is_empty() {
            out.push(' ');
            continue;
        }
        let line = line
            .trim_start_matches(|c: char| {
                matches!(c, '#' | '>' | '-' | '*' | '•' | '+') || c.is_whitespace()
            })
            .to_string();
        // A numbered list marker "3. " is not a sentence of its own.
        let line = match line.split_once(". ") {
            Some((n, rest))
                if !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()) =>
            {
                rest.to_string()
            }
            _ => line,
        };
        out.push_str(&line);
        // A line without end punctuation is still a pause.
        if !line_ends_sentence(&out) {
            out.push('.');
        }
        out.push(' ');
    }
    let mut s = strip_markdown_links(&out);
    s = replace_urls(&s);
    s = s.replace(['`', '*', '_', '~', '|'], "");
    s = s.replace('&', " and ").replace('%', " percent");
    let s: String = s.chars().filter(|c| !is_emoji(*c)).collect();
    let spaced = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Where a symbol or emoji was dropped it leaves a space before the mark.
    spaced
        .replace(" .", ".")
        .replace(" ,", ",")
        .replace(" !", "!")
        .replace(" ?", "?")
}

fn line_ends_sentence(s: &str) -> bool {
    matches!(
        s.trim_end().chars().last(),
        Some('.' | '!' | '?' | ':' | ';' | '…')
    )
}

fn strip_markdown_links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(')')) {
            (Some(mid), Some(close)) if mid < close => {
                out.push_str(&rest[..open]);
                out.push_str(&after[..mid]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn replace_urls(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let bare = w.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | '"' | '\''));
            if bare.starts_with("http://")
                || bare.starts_with("https://")
                || bare.starts_with("www.")
            {
                "a link"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D | 0x2B50 | 0x2B06..=0x2B07)
}

/// Pieces to speak one after another: sentences, each at most `max` characters
/// (a long one is cut at a comma or space), so the first is ready quickly.
pub fn sentences(text: &str, max: usize) -> Vec<String> {
    const ABBREV: &[&str] = &["e.g", "i.e", "mr", "mrs", "ms", "dr", "vs", "etc", "no"];
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…') {
            let next = chars.get(i + 1).copied();
            let ends = next.is_none_or(|n| n.is_whitespace());
            let word = cur
                .trim_end_matches(['.', '!', '?'])
                .rsplit(' ')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let digit_before = cur
                .trim_end_matches('.')
                .chars()
                .last()
                .is_some_and(|d| d.is_ascii_digit());
            let is_abbrev = c == '.'
                && (ABBREV.contains(&word.as_str())
                    || (digit_before && next.is_some_and(|n| n.is_ascii_digit())));
            if ends && !is_abbrev {
                out.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    let mut pieces = Vec::new();
    for s in out {
        let mut s = s.trim().to_string();
        while s.chars().count() > max {
            let head: String = s.chars().take(max).collect();
            let cut = head
                .rfind(", ")
                .map(|p| p + 1)
                .or_else(|| head.rfind(' '))
                .unwrap_or(head.len());
            let (a, b) = s.split_at(cut);
            pieces.push(a.trim().to_string());
            s = b.trim().to_string();
        }
        if !s.is_empty() {
            pieces.push(s);
        }
    }
    pieces
        .into_iter()
        .filter(|p| p.chars().any(char::is_alphanumeric))
        .collect()
}

// ---------------------------------------------------------------- audio

/// 16-bit PCM mono samples and their rate, as read from a WAV.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<i16>,
}

pub fn read_wav(bytes: &[u8]) -> Result<Pcm, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let (mut rate, mut channels, mut bits, mut data): (u32, u16, u16, Option<&[u8]>) =
        (0, 0, 0, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = bytes
            .get(at + 8..(at + 8 + len).min(bytes.len()))
            .unwrap_or(&[]);
        match id {
            b"fmt " if body.len() >= 16 => {
                channels = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes([body[14], body[15]]);
            }
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    let data = data.ok_or("the WAV has no audio")?;
    if bits != 16 || channels == 0 || rate == 0 {
        return Err(format!("expected 16-bit PCM, got {bits}-bit"));
    }
    let all: Vec<i16> = data
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    // Mono: the first channel.
    let samples = all.chunks(channels as usize).map(|f| f[0]).collect();
    Ok(Pcm { rate, samples })
}

pub fn write_wav(p: &Pcm) -> Vec<u8> {
    let data_len = (p.samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&p.rate.to_le_bytes());
    b.extend_from_slice(&(p.rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in &p.samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// Brings a sentence to a steady, comfortable level: its speech (not its
/// silences) at `target_rms` of full scale, never so much gain that the peak
/// passes `peak_limit`, with a few milliseconds of fade so a sentence never
/// clicks. Engines differ a lot (one peaks at full scale, another at a quarter).
pub fn normalize(p: &mut Pcm, target_rms: f32, peak_limit: f32) {
    let full = i16::MAX as f32;
    let loud: Vec<f32> = p
        .samples
        .iter()
        .map(|&s| s as f32 / full)
        .filter(|s| s.abs() > 0.01)
        .collect();
    if loud.is_empty() {
        return;
    }
    let rms = (loud.iter().map(|s| s * s).sum::<f32>() / loud.len() as f32).sqrt();
    let peak = p
        .samples
        .iter()
        .map(|&s| (s as f32 / full).abs())
        .fold(0.0, f32::max);
    let gain = (target_rms / rms).min(peak_limit / peak).clamp(0.05, 8.0);
    let fade = (p.rate as usize / 100).max(1);
    let n = p.samples.len();
    for (i, s) in p.samples.iter_mut().enumerate() {
        let edge = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
        *s = (*s as f32 * gain * edge).round().clamp(-full, full) as i16;
    }
}

// ---------------------------------------------------------------- the work

/// Makes the audio for one sentence (the engine, in production).
pub trait Synth: Send + Sync {
    fn synth(&self, text: &str) -> Result<Pcm, String>;
    /// How many sentences may be made at the same time (a slow voice makes
    /// several at once to keep ahead of the speech).
    fn workers(&self) -> usize {
        1
    }
}

/// Plays one prepared WAV file; `cancel` stops it early.
pub trait Player: Send + Sync {
    fn play(&self, wav: &Path, cancel: &AtomicBool) -> Result<(), String>;
}

/// What happened: how many sentences were spoken and whether it was stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spoken {
    pub sentences: usize,
    pub stopped: bool,
}

/// Speaks `text` sentence by sentence: the next sentences are made while the
/// current one plays (as many at once as the voice's `workers`), so speech starts
/// after one sentence's work, not the whole answer's, and is played in order.
/// Stops at once when `cancel` is set.
pub fn speak(
    text: &str,
    synth: Arc<dyn Synth>,
    player: &dyn Player,
    work_dir: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<Spoken, String> {
    let parts = sentences(&clean_for_speech(text), 220);
    if parts.is_empty() {
        return Ok(Spoken {
            sentences: 0,
            stopped: false,
        });
    }
    std::fs::create_dir_all(work_dir).map_err(|e| e.to_string())?;
    let n = parts.len();
    let workers = synth.workers().max(1);
    // One channel per sentence, so a sentence made early still waits its turn.
    type Made = Result<PathBuf, String>;
    let (senders, receivers): (Vec<_>, Vec<_>) = (0..n).map(|_| mpsc::channel::<Made>()).unzip();
    let senders: Arc<Vec<Mutex<mpsc::Sender<Made>>>> =
        Arc::new(senders.into_iter().map(Mutex::new).collect());
    let parts = Arc::new(parts);
    let next = Arc::new(AtomicUsize::new(0));
    // The sentence being played: no worker starts one more than `workers` ahead of it.
    let playing = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (synth, parts, senders) = (synth.clone(), parts.clone(), senders.clone());
            let (next, playing, cancel) = (next.clone(), playing.clone(), cancel.clone());
            let dir = work_dir.to_path_buf();
            std::thread::spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= parts.len() {
                    break;
                }
                while i > playing.load(Ordering::SeqCst) + workers && !cancel.load(Ordering::SeqCst)
                {
                    std::thread::sleep(Duration::from_millis(30));
                }
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let made = synth.synth(&parts[i]).map(|mut pcm| {
                    normalize(&mut pcm, 0.10, 0.89);
                    pcm
                });
                let item = made.and_then(|pcm| {
                    let path = dir.join(format!("part-{i}.wav"));
                    std::fs::write(&path, write_wav(&pcm)).map_err(|e| e.to_string())?;
                    Ok(path)
                });
                let failed = item.is_err();
                let _ = senders[i].lock().map(|tx| tx.send(item));
                if failed {
                    break;
                }
            })
        })
        .collect();
    let (mut spoken, mut error) = (0, None);
    'sentences: for (i, rx) in receivers.into_iter().enumerate() {
        let item = loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(item) => break item,
                Err(mpsc::RecvTimeoutError::Timeout) if !cancel.load(Ordering::SeqCst) => {}
                Err(_) => break 'sentences,
            }
        };
        playing.store(i, Ordering::SeqCst);
        match item {
            Ok(path) => {
                if !cancel.load(Ordering::SeqCst) {
                    if let Err(e) = player.play(&path, &cancel) {
                        error = Some(e);
                        cancel.store(true, Ordering::SeqCst);
                    } else {
                        spoken += 1;
                    }
                }
                let _ = std::fs::remove_file(path);
            }
            Err(e) => {
                error = Some(e);
                cancel.store(true, Ordering::SeqCst);
            }
        }
        if cancel.load(Ordering::SeqCst) {
            break;
        }
    }
    // Anything still being made is not wanted any more: cancel is set on every early exit.
    for h in handles {
        let _ = h.join();
    }
    // Sentences made but never played (a stop, a failure) leave their files behind.
    for i in 0..n {
        let _ = std::fs::remove_file(work_dir.join(format!("part-{i}.wav")));
    }
    match error {
        Some(e) if spoken == 0 => Err(e),
        _ => Ok(Spoken {
            sentences: spoken,
            stopped: cancel.load(Ordering::SeqCst),
        }),
    }
}

// ---------------------------------------------------------------- the real engine and player

/// The sherpa-onnx engine, one process per sentence (it loads in well under a second).
pub struct EngineSynth {
    pub voice: Voice,
    pub work_dir: PathBuf,
}

impl Synth for EngineSynth {
    /// Kokoro is slower than real time on a laptop CPU: three sentences at once.
    fn workers(&self) -> usize {
        if self.voice.kind == Kind::Kokoro {
            3
        } else {
            1
        }
    }

    fn synth(&self, text: &str) -> Result<Pcm, String> {
        let out = self.work_dir.join(format!(
            "engine-{}-{}.wav",
            std::process::id(),
            rand::random::<u32>()
        ));
        let mut cmd = Command::new(&self.voice.engine);
        cmd.args(engine_args(&self.voice, text, &out))
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(lib) = self
            .voice
            .engine
            .parent()
            .and_then(Path::parent)
            .map(|d| d.join("lib"))
        {
            if lib.is_dir() {
                #[cfg(not(windows))]
                cmd.env("LD_LIBRARY_PATH", &lib);
            }
        }
        let done = cmd
            .output()
            .map_err(|e| format!("couldn't run the voice engine: {e}"))?;
        let bytes = std::fs::read(&out).map_err(|_| "the voice engine made no audio".to_string());
        let _ = std::fs::remove_file(&out);
        if !done.status.success() {
            return Err(engine_failure(&done.stderr));
        }
        read_wav(&bytes?)
    }
}

/// What to tell the person when the engine exits with an error: its own last
/// line (a missing model file, a bad library) and how to reinstall the voice.
fn engine_failure(stderr: &[u8]) -> String {
    let last = String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect::<String>());
    match last {
        Some(l) => {
            format!("the voice engine failed: {l} (reinstall the voice: friday voice speak setup)")
        }
        None => "the voice engine failed (reinstall the voice: friday voice speak setup)".into(),
    }
}

/// tiny-voca, run in this process (no program to start per sentence).
pub struct TinyVocaSynth {
    voca: crate::tinyvoca::Voca,
    voice: usize,
    speed: f32,
}

impl TinyVocaSynth {
    pub fn load(v: &Voice) -> Result<Self, String> {
        let root = v
            .engine
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        // The ONNX Runtime that comes with the engine package: bin/ on Windows, lib/ elsewhere.
        let runtime = if cfg!(windows) {
            root.join("bin").join("onnxruntime.dll")
        } else {
            root.join("lib").join("libonnxruntime.so")
        };
        Ok(Self {
            voca: crate::tinyvoca::Voca::load(&v.model, &runtime, v.threads as usize)?,
            voice: v.speaker as usize,
            speed: v.speed,
        })
    }
}

impl Synth for TinyVocaSynth {
    fn synth(&self, text: &str) -> Result<Pcm, String> {
        let mut samples = self.voca.speak(text, self.voice, self.speed)?;
        if samples.is_empty() {
            // Nothing to read in this sentence: a moment of silence keeps the WAV valid for the player.
            samples = vec![0; crate::tinyvoca::SAMPLE_RATE as usize / 20];
        }
        Ok(Pcm {
            rate: crate::tinyvoca::SAMPLE_RATE,
            samples,
        })
    }
}

/// Plays through the system's audio: PipeWire, PulseAudio or ALSA on Linux,
/// Windows' own sound player on Windows. With `dump` set (`FRIDAY_SAY_DUMP`
/// names a folder), each sentence is saved there instead of played, which is
/// how the speech is checked on a machine with no speakers.
pub struct SystemPlayer {
    pub dump: Option<PathBuf>,
}

fn play_command(wav: &Path) -> Option<Command> {
    #[cfg(windows)]
    {
        let path = wav.to_string_lossy().replace('\'', "''");
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(New-Object Media.SoundPlayer '{path}').PlaySync()"),
        ]);
        Some(c)
    }
    #[cfg(not(windows))]
    {
        for (tool, args) in [
            ("pw-play", vec![]),
            ("paplay", vec![]),
            ("aplay", vec!["-q"]),
        ] {
            let found = std::env::var_os("PATH")
                .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()));
            if found {
                let mut c = Command::new(tool);
                c.args(args).arg(wav);
                return Some(c);
            }
        }
        None
    }
}

impl Player for SystemPlayer {
    fn play(&self, wav: &Path, cancel: &AtomicBool) -> Result<(), String> {
        if let Some(dir) = &self.dump {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let name = format!(
                "{}-{}",
                std::process::id(),
                wav.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("part.wav")
            );
            return std::fs::copy(wav, dir.join(name))
                .map(|_| ())
                .map_err(|e| e.to_string());
        }
        let mut cmd =
            play_command(wav).ok_or("no audio player found (pw-play, paplay or aplay)")?;
        let mut child: Child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

/// One speaker at a time: where the running `say` records its process id, so a
/// new one (or `--stop`) can end it.
pub fn pid_file() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("friday-say.pid")
}

/// Whether an answer is being spoken now: a `say` recorded its pid and is alive.
pub fn speaking() -> bool {
    let Some(pid) = std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    else {
        return false;
    };
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists() && is_friday_process(pid)
    }
    #[cfg(windows)]
    {
        let _ = pid;
        true
    }
}

/// Ends a running `say` (a new answer replaces the old one; Stop and a key press end it).
pub fn stop_running() -> bool {
    let path = pid_file();
    let Some(pid) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    else {
        return false;
    };
    let _ = std::fs::remove_file(&path);
    if pid == std::process::id() {
        return false;
    }
    #[cfg(windows)]
    let ok = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    #[cfg(not(windows))]
    let ok = {
        let alive = || {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        // A pid file left by a speaker that died: the number may belong to anything now.
        if !alive() || !is_friday_process(pid) {
            return false;
        }
        // The speaker sees its pid file go and stops itself, the player with it; killing
        // it outright would leave the sentence being played running.
        let mut ended = false;
        for _ in 0..25 {
            std::thread::sleep(Duration::from_millis(40));
            if !alive() {
                ended = true;
                break;
            }
        }
        ended
            || Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status()
                .is_ok_and(|s| s.success())
    };
    ok
}

/// Whether `pid` is one of FRIDAY's own programs (where that can be told).
#[cfg(not(windows))]
fn is_friday_process(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map_or(true, |c| c.starts_with("friday"))
}

/// Stops the speech when this process's pid file is taken away or rewritten: that is how
/// `stop_running` and a newer `say` ask for it. Ends once `done` is set.
fn watch_pid_file(path: PathBuf, cancel: Arc<AtomicBool>, done: Arc<AtomicBool>) {
    let me = std::process::id().to_string();
    std::thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
            if !std::fs::read_to_string(&path).is_ok_and(|t| t.trim() == me) {
                cancel.store(true, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    });
}

/// Speaks `text` with the installed voice. Replaces whatever is being said.
pub fn say(text: &str) -> Result<Spoken, String> {
    let dir = voice_dir();
    let voice = detect(&dir).ok_or("FRIDAY's voice isn't installed (friday voice speak setup)")?;
    let work = std::env::temp_dir().join(format!("friday-say-{}", std::process::id()));
    // Loaded before anything is written, so a voice that can't load leaves nothing behind.
    let synth: Arc<dyn Synth> = match voice.kind {
        Kind::TinyVoca => Arc::new(TinyVocaSynth::load(&voice)?),
        _ => Arc::new(EngineSynth {
            voice,
            work_dir: work.clone(),
        }),
    };
    stop_running();
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    if let Ok(mut f) = std::fs::File::create(pid_file()) {
        let _ = write!(f, "{}", std::process::id());
        watch_pid_file(pid_file(), cancel.clone(), done.clone());
    }
    let player = SystemPlayer {
        dump: std::env::var_os("FRIDAY_SAY_DUMP").map(PathBuf::from),
    };
    let result = speak(text, synth, &player, &work, cancel);
    done.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(&work);
    if std::fs::read_to_string(pid_file())
        .ok()
        .is_some_and(|t| t.trim() == std::process::id().to_string())
    {
        let _ = std::fs::remove_file(pid_file());
    }
    result
}

// ---------------------------------------------------------------- installing

/// What gets downloaded, pinned by sha256 (the digests GitHub publishes for these
/// release files of the Apache-2.0 sherpa-onnx project).
const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
#[cfg(not(windows))]
const ENGINE: (&str, &str, &str) = (
    "v1.13.8",
    "sherpa-onnx-v1.13.8-linux-x64-shared.tar.bz2",
    "c0bdb7907d3a74bba1d55d22bf4d9fa75586cf1530614ebe88a27b9118e015c4",
);
#[cfg(windows)]
const ENGINE: (&str, &str, &str) = (
    "v1.13.8",
    "sherpa-onnx-v1.13.8-win-x64-shared-MT-Release.tar.bz2",
    "6dffdc715a4465b989446a6105265d2cb345e7101591a17d35534b6758f6e8df",
);

/// tiny-voca's files (Apache-2.0, n4ze3m/tiny-voca on Hugging Face) at one
/// revision: (path, sha256). The phonemizer folder holds CMUdict and the
/// Wikipedia homograph data with their licences.
const TINYVOCA: &str =
    "https://huggingface.co/n4ze3m/tiny-voca/resolve/d94d2a0338fd1e0eab7dd896ec0ad23d8cdf7c14";
const TINYVOCA_FILES: [(&str, &str); 9] = [
    (
        "onnx/model.onnx",
        "fe6d918791a15b82e37465981d7f27a070e46b46635e5a3482fc8dff87a1ae46",
    ),
    (
        "phonemizer/align.json",
        "864763dbd558a864950266b2c30627f1cad203e75062ee842ee63c653e823b75",
    ),
    (
        "phonemizer/g2p.json",
        "c518fd839ea0109c0b2b4f778c17506bf0d7653557b8b8c97a4c8dd9883d4432",
    ),
    (
        "phonemizer/g2p.npz",
        "eb3fd519ff00e72d850928d2fccb968adc36134bca2f3bda09de7e794c2ed01b",
    ),
    (
        "phonemizer/homographs.json",
        "9c3a96fcdf79fa27d1b3686dc6a507c37e4f754966928608ee9ddb617ba39192",
    ),
    (
        "phonemizer/lexicon.tsv",
        "3fc930345c6b3cabc533981c4b212fa309eec83913fcdbd42919338270d64b06",
    ),
    (
        "phonemizer/LICENSE.cmudict",
        "bd4ce8e44170a5f9f481310ca85c51de3c4f851a65e679b40e603b143bd3542a",
    ),
    (
        "phonemizer/LICENSE.homograph-data",
        "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
    ),
    (
        "LICENSE",
        "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
    ),
];

/// The model folder of tiny-voca: `model.onnx`, `phonemizer/` and the licences.
fn fetch_tinyvoca(into: &Path) -> Result<(), String> {
    for (path, sha) in TINYVOCA_FILES {
        let name = Path::new(path).file_name().unwrap_or_default();
        let dest = into
            .join(path.rsplit_once('/').map_or("", |(dir, _)| dir))
            .join(name);
        std::fs::create_dir_all(dest.parent().unwrap_or(into)).map_err(|e| e.to_string())?;
        download(&format!("{TINYVOCA}/{path}"), &dest, sha)?;
    }
    // The model sits at the top, as `Voca::load` looks for it.
    std::fs::rename(into.join("onnx/model.onnx"), into.join("model.onnx"))
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir(into.join("onnx"));
    Ok(())
}

fn model_pin(kind: Kind) -> (&'static str, &'static str, &'static str) {
    match kind {
        Kind::TinyVoca => unreachable!("tiny-voca is not an engine model archive"),
        Kind::Supertonic => (
            "tts-models",
            "sherpa-onnx-supertonic-tts-int8-2026-03-06.tar.bz2",
            "8c74359f63edd5045d47747f65331f0f6dbcbc91d7e898dd756d631295fe3259",
        ),
        Kind::Kitten => (
            "tts-models",
            "kitten-nano-en-v0_8-int8.tar.bz2",
            "6fa5be852612ce761094ba74ee6123b4fc4acfefa79bf64dc63acae4a83af2fd",
        ),
        Kind::Kokoro => (
            "tts-models",
            "kokoro-int8-multi-lang-v1_0.tar.bz2",
            "72c347230107ef8a31098ec4d468a605244beef5f1ed00db97efa13518800653",
        ),
    }
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads one pinned file with `curl` and refuses it unless it matches.
fn fetch(pin: (&str, &str, &str), into: &Path) -> Result<PathBuf, String> {
    let (tag, file, sha) = pin;
    let dest = into.join(file);
    download(&format!("{RELEASES}/{tag}/{file}"), &dest, sha)?;
    Ok(dest)
}

/// Downloads `url` to `dest` with `curl` and removes it unless it matches `sha`.
pub(crate) fn download(url: &str, dest: &Path, sha: &str) -> Result<(), String> {
    let file = dest.file_name().unwrap_or_default().to_string_lossy();
    let status = Command::new("curl")
        .args(["-fL", "--silent", "--show-error", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if !status.success() {
        return Err(format!("couldn't download {file}"));
    }
    if sha256_hex(dest)? != sha {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "{file} doesn't match its checksum; not installing it"
        ));
    }
    Ok(())
}

fn untar(archive: &Path, into: &Path) -> Result<(), String> {
    let status = Command::new("tar")
        .arg("xjf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .map_err(|e| format!("couldn't run tar: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "couldn't unpack the download".to_string())
}

/// The one folder an unpacked archive contains.
fn only_dir(of: &Path) -> Result<PathBuf, String> {
    let mut dirs = std::fs::read_dir(of)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir());
    dirs.next()
        .ok_or_else(|| "the download held no folder".to_string())
}

fn copy_files(from: &Path, to: &Path, keep: impl Fn(&str) -> bool) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let Ok(read) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for entry in read.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && keep(&name) {
            std::fs::copy(entry.path(), to.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The folders of an installed voice that a new install replaces.
const VOICE_PARTS: [&str; 3] = ["bin", "lib", "model"];

/// Moves each of `parts` from `new` into `dir`, keeping what was there until
/// every one is in place. If any step fails, what was placed is removed and
/// the old parts are put back, so a failed reinstall never costs the working
/// voice.
fn swap_in(new: &Path, dir: &Path, parts: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let aside = |p: &str| dir.join(format!(".old-{}", p.replace('/', "-")));
    for p in parts {
        let _ = std::fs::remove_dir_all(aside(p));
    }
    let mut moved: Vec<&str> = Vec::new();
    let mut placed: Vec<&str> = Vec::new();
    let result = (|| -> Result<(), String> {
        for p in parts {
            if dir.join(p).exists() {
                std::fs::rename(dir.join(p), aside(p)).map_err(|e| e.to_string())?;
                moved.push(p);
            }
            if new.join(p).exists() {
                std::fs::rename(new.join(p), dir.join(p)).map_err(|e| e.to_string())?;
                placed.push(p);
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for p in &placed {
            let _ = std::fs::remove_dir_all(dir.join(p));
        }
        for p in &moved {
            let _ = std::fs::rename(aside(p), dir.join(p));
        }
    } else {
        for p in parts {
            let _ = std::fs::remove_dir_all(aside(p));
        }
    }
    result
}

/// Installs the voice into `dir`: the pinned engine and the chosen model, then
/// `settings.json`. Anything already there is replaced; a failure leaves the old
/// voice as it was.
pub fn setup(dir: &Path, kind: Kind) -> Result<(), String> {
    // A voice being read aloud holds the engine's libraries open (on Windows that blocks replacing them).
    stop_running();
    let work = std::env::temp_dir().join(format!("friday-voice-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = (|| {
        let engine_tar = fetch(ENGINE, &work)?;
        let (engine_out, model_out) = (work.join("engine"), work.join("model"));
        std::fs::create_dir_all(&engine_out).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&model_out).map_err(|e| e.to_string())?;
        untar(&engine_tar, &engine_out)?;
        let engine_root = only_dir(&engine_out)?;
        // tiny-voca is a set of files; the others are one archive with a folder in it.
        let model_root = if kind == Kind::TinyVoca {
            let root = model_out.join("tiny-voca");
            fetch_tinyvoca(&root)?;
            root
        } else {
            untar(&fetch(model_pin(kind), &work)?, &model_out)?;
            only_dir(&model_out)?
        };
        let new = work.join("new");
        // The engine program, and what it loads: the DLLs beside it on Windows, lib/ on Linux.
        copy_files(&engine_root.join("bin"), &new.join("bin"), |n| {
            n == engine_name() || n.ends_with(".dll")
        })?;
        copy_files(&engine_root.join("lib"), &new.join("lib"), |n| {
            n.contains(".so")
        })?;
        std::fs::rename(&model_root, new.join("model")).map_err(|e| e.to_string())?;
        if !new.join("bin").join(engine_name()).is_file() {
            return Err("the engine download held no speech program".into());
        }
        let kind_name = match kind {
            Kind::Kitten => "kitten",
            Kind::Kokoro => "kokoro",
            Kind::Supertonic => "supertonic",
            Kind::TinyVoca => "tinyvoca",
        };
        std::fs::write(
            new.join("settings.json"),
            format!("{{\"kind\": \"{kind_name}\", \"speaker\": 0}}\n"),
        )
        .map_err(|e| e.to_string())?;
        swap_in(&new, dir, &VOICE_PARTS)?;
        std::fs::copy(new.join("settings.json"), dir.join("settings.json"))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe = dir.join("bin").join(engine_name());
            let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

pub fn kind_from_name(name: &str) -> Option<Kind> {
    Kind::parse(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn the_speaker_stops_when_its_pid_file_is_taken_or_rewritten() {
        let dir = std::env::temp_dir().join(format!("friday-pid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("friday-say.pid");
        let wait_for = |flag: &AtomicBool| {
            for _ in 0..100 {
                if flag.load(Ordering::SeqCst) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        };

        // Rewritten by a newer speaker: stop.
        std::fs::write(&path, std::process::id().to_string()).unwrap();
        let (cancel, done) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        watch_pid_file(path.clone(), cancel.clone(), done.clone());
        std::thread::sleep(Duration::from_millis(150));
        assert!(!cancel.load(Ordering::SeqCst), "still its own file");
        std::fs::write(&path, "1").unwrap();
        assert!(wait_for(&cancel));

        // Taken away by `stop_running`: stop.
        std::fs::write(&path, std::process::id().to_string()).unwrap();
        let (cancel, done2) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        watch_pid_file(path.clone(), cancel.clone(), done2.clone());
        std::fs::remove_file(&path).unwrap();
        assert!(wait_for(&cancel));

        done.store(true, Ordering::SeqCst);
        done2.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tone(rate: u32, secs: f32, amp: f32) -> Pcm {
        let n = (rate as f32 * secs) as usize;
        Pcm {
            rate,
            samples: (0..n)
                .map(|i| ((i as f32 * 0.2).sin() * amp * 32767.0) as i16)
                .collect(),
        }
    }

    #[test]
    fn markdown_code_links_and_symbols_become_speakable() {
        let t = "## Result\n- Battery is at **40%** & charging\n- See [the docs](https://example.com/x) or https://example.org now\n```rust\nlet x = 1;\n```\nDone 🎉";
        let s = clean_for_speech(t);
        assert!(s.contains("Battery is at 40 percent and charging"), "{s}");
        assert!(s.contains("See the docs or a link now"), "{s}");
        assert!(s.contains("There is some code on screen."), "{s}");
        assert!(!s.contains("let x"), "code is not read out: {s}");
        assert!(
            !s.contains('*') && !s.contains('#') && !s.contains('🎉'),
            "{s}"
        );
        assert!(s.ends_with("Done."), "{s}");
    }

    #[test]
    fn a_numbered_list_marker_is_not_a_sentence() {
        let s = clean_for_speech("1. Open the app\n2. Press play");
        assert_eq!(s, "Open the app. Press play.");
    }

    #[test]
    fn sentences_break_at_ends_but_not_at_abbreviations_or_decimals() {
        let v = sentences(
            "It is 3.5 volts, e.g. low. Is that fine? Yes! Dr. Smith agrees.",
            220,
        );
        assert_eq!(
            v,
            [
                "It is 3.5 volts, e.g. low.",
                "Is that fine?",
                "Yes!",
                "Dr. Smith agrees."
            ]
        );
    }

    #[test]
    fn a_long_sentence_is_cut_at_a_comma_or_space_and_nothing_is_lost() {
        let long = "word ".repeat(80).trim().to_string() + ".";
        let v = sentences(&long, 100);
        assert!(v.len() >= 4, "{}", v.len());
        assert!(v.iter().all(|p| p.chars().count() <= 100));
        assert_eq!(v.join(" ").split_whitespace().count(), 80);
        let commas = "alpha beta gamma delta, epsilon zeta eta theta iota kappa lambda mu, nu xi omicron pi rho sigma tau.";
        let v = sentences(commas, 60);
        assert!(v[0].ends_with(','), "{v:?}");
    }

    #[test]
    fn pieces_with_nothing_to_say_are_dropped() {
        assert!(sentences("... !!! ", 100).is_empty());
        assert!(sentences("   ", 100).is_empty());
    }

    #[test]
    fn wav_round_trips_and_other_formats_are_refused() {
        let p = tone(24000, 0.1, 0.5);
        assert_eq!(read_wav(&write_wav(&p)).unwrap(), p);
        assert!(read_wav(b"nope").is_err());
        let mut bad = write_wav(&p);
        bad[34] = 8; // 8-bit
        assert!(read_wav(&bad).is_err());
    }

    #[test]
    fn engines_that_come_out_loud_or_quiet_end_at_the_same_comfortable_level() {
        let level = |p: &Pcm| {
            let loud: Vec<f32> = p
                .samples
                .iter()
                .map(|&s| s as f32 / 32767.0)
                .filter(|s| s.abs() > 0.01)
                .collect();
            (loud.iter().map(|s| s * s).sum::<f32>() / loud.len() as f32).sqrt()
        };
        let peak = |p: &Pcm| {
            p.samples
                .iter()
                .map(|&s| (s as f32 / 32767.0).abs())
                .fold(0.0, f32::max)
        };
        let (mut hot, mut quiet) = (tone(24000, 0.5, 1.0), tone(44100, 0.5, 0.1));
        normalize(&mut hot, 0.10, 0.89);
        normalize(&mut quiet, 0.10, 0.89);
        for p in [&hot, &quiet] {
            assert!((level(p) - 0.10).abs() < 0.02, "rms {}", level(p));
            assert!(peak(p) <= 0.9, "peak {}", peak(p));
        }
        // A sentence fades in and out: no click at either edge.
        assert_eq!(hot.samples[0], 0);
        assert!(hot.samples[hot.samples.len() - 1].abs() < 400);
    }

    #[test]
    fn silence_is_left_alone() {
        let mut p = Pcm {
            rate: 24000,
            samples: vec![0; 1000],
        };
        normalize(&mut p, 0.1, 0.89);
        assert!(p.samples.iter().all(|&s| s == 0));
    }

    fn voice(kind: Kind) -> Voice {
        Voice {
            engine: "/e/bin/x".into(),
            model: "/m".into(),
            kind,
            speaker: 3,
            speed: 1.25,
            threads: 2,
        }
    }

    #[test]
    fn kokoro_defaults_to_bella_and_takes_its_voices_by_position() {
        let bella = engine_args(&voice(Kind::Kokoro), "hi", Path::new("/o.wav"));
        // voice() has speaker 3: position 3 is Sarah, speaker id 9.
        assert!(bella.contains(&"--sid=9".to_string()), "{bella:?}");
        let mut v = voice(Kind::Kokoro);
        v.speaker = 0;
        let a = engine_args(&v, "hi", Path::new("/o.wav"));
        assert!(
            a.contains(&"--sid=2".to_string()),
            "position 0 is Bella: {a:?}"
        );
        v.speaker = 63;
        let last = engine_args(&v, "hi", Path::new("/o.wav"));
        assert!(
            last.contains(&"--sid=22".to_string()),
            "past the end is the last voice: {last:?}"
        );
        let model = Path::new("/m").join("model.int8.onnx");
        assert!(a.contains(&format!("--kokoro-model={}", model.display())));
        assert!(a
            .iter()
            .any(|x| x.starts_with("--kokoro-lexicon=") && !x.contains("zh")));
        assert!(
            a.contains(&"--kokoro-length-scale=0.8".to_string()),
            "{a:?}"
        );
        assert_eq!(a.last().unwrap(), "hi");
    }

    #[test]
    fn every_kind_names_its_voices() {
        assert_eq!(voice_names(Kind::Kokoro)[0], "Bella");
        assert_eq!(voice_names(Kind::Kokoro).len(), KOKORO_VOICES.len());
        assert_eq!(voice_names(Kind::Kitten).len(), 8);
        assert_eq!(voice_names(Kind::Supertonic).len(), 10);
        assert_eq!(kind_from_name("Kokoro"), Some(Kind::Kokoro));
        assert_eq!(voice_names(Kind::TinyVoca), ["Female", "Male"]);
        assert_eq!(kind_from_name("tinyvoca"), Some(Kind::TinyVoca));
        assert_eq!(kind_from_name("tiny-voca"), Some(Kind::TinyVoca));
    }

    #[test]
    fn tiny_voca_runs_in_this_process_and_is_chosen_in_settings() {
        // It has no engine command line: the text is all there is.
        assert_eq!(
            engine_args(&voice(Kind::TinyVoca), "hi", Path::new("/o.wav"))
                .iter()
                .filter(|a| a.starts_with("--kokoro") || a.starts_with("--kitten"))
                .count(),
            0
        );
        let dir = std::env::temp_dir().join(format!("friday-tiny-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("model")).unwrap();
        std::fs::write(dir.join("bin").join(engine_name()), "").unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"kind":"tinyvoca","speaker":1}"#,
        )
        .unwrap();
        let v = detect(&dir).unwrap();
        assert_eq!((v.kind, v.speaker), (Kind::TinyVoca, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_model_gets_its_own_flags_and_the_text_comes_last() {
        let s = engine_args(&voice(Kind::Supertonic), "hello there", Path::new("/o.wav"));
        assert!(s.contains(&"--sid=3".to_string()) && s.contains(&"--speed=1.25".to_string()));
        let vocoder = Path::new("/m").join("vocoder.int8.onnx");
        assert!(s.contains(&format!("--supertonic-vocoder={}", vocoder.display())));
        assert_eq!(s.last().unwrap(), "hello there");
        let k = engine_args(&voice(Kind::Kitten), "hi", Path::new("/o.wav"));
        let model = Path::new("/m").join("model.int8.onnx");
        assert!(k.contains(&format!("--kitten-model={}", model.display())));
        assert!(
            k.contains(&"--kitten-length-scale=0.8".to_string()),
            "{k:?}"
        );
    }

    #[test]
    fn a_voice_is_found_by_its_files_and_settings() {
        let dir = std::env::temp_dir().join(format!("friday-voice-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(detect(&dir).is_none());
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("model")).unwrap();
        std::fs::write(dir.join("bin").join(engine_name()), "").unwrap();
        // A model of unknown kind is not guessed at.
        assert!(detect(&dir).is_none());
        std::fs::write(dir.join("model/duration_predictor.int8.onnx"), "").unwrap();
        assert_eq!(detect(&dir).unwrap().kind, Kind::Supertonic);
        std::fs::write(
            dir.join("settings.json"),
            r#"{"kind":"kitten","speaker":5,"speed":9,"threads":99}"#,
        )
        .unwrap();
        let v = detect(&dir).unwrap();
        assert_eq!((v.kind, v.speaker, v.threads), (Kind::Kitten, 5, 8));
        assert!(
            (v.speed - 1.6).abs() < 1e-6,
            "speed is kept within a natural range"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct FakeSynth {
        made: Mutex<Vec<String>>,
        fail_on: Option<&'static str>,
    }
    impl Synth for FakeSynth {
        fn synth(&self, text: &str) -> Result<Pcm, String> {
            if self.fail_on.is_some_and(|f| text.contains(f)) {
                return Err("engine broke".into());
            }
            self.made.lock().unwrap().push(text.to_string());
            Ok(tone(24000, 0.05, 0.3))
        }
    }
    struct FakePlayer {
        played: Mutex<Vec<String>>,
        stop_after: Option<usize>,
    }
    impl Player for FakePlayer {
        fn play(&self, wav: &Path, cancel: &AtomicBool) -> Result<(), String> {
            assert!(wav.is_file(), "the file exists while it plays");
            let mut played = self.played.lock().unwrap();
            played.push(wav.file_name().unwrap().to_string_lossy().into_owned());
            if self.stop_after == Some(played.len()) {
                cancel.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
    }
    /// Earlier sentences take longer than later ones, three at a time.
    struct SlowFirstSynth;
    impl Synth for SlowFirstSynth {
        fn workers(&self) -> usize {
            3
        }
        fn synth(&self, text: &str) -> Result<Pcm, String> {
            let n: u64 = text
                .split_whitespace()
                .next()
                .and_then(|w| w.trim_end_matches('.').parse().ok())
                .unwrap_or(0);
            std::thread::sleep(Duration::from_millis(60 * (6 - n.min(5))));
            Ok(tone(24000, 0.05, 0.3))
        }
    }

    #[test]
    fn sentences_made_at_the_same_time_still_play_in_order() {
        let (dir, player) = (
            work(),
            FakePlayer {
                played: Mutex::new(vec![]),
                stop_after: None,
            },
        );
        let started = std::time::Instant::now();
        let spoken = speak(
            "1 First. 2 Second. 3 Third. 4 Fourth. 5 Fifth.",
            Arc::new(SlowFirstSynth),
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(spoken.sentences, 5);
        let order = player.played.lock().unwrap().clone();
        assert_eq!(
            order,
            [
                "part-0.wav",
                "part-1.wav",
                "part-2.wav",
                "part-3.wav",
                "part-4.wav"
            ]
        );
        // One by one the five would take 60 * (5+4+3+2+1) = 900 ms.
        assert!(
            started.elapsed() < Duration::from_millis(800),
            "{:?}",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stopping_while_several_are_made_at_once_leaves_no_files() {
        let (player, dir) = (
            FakePlayer {
                played: Mutex::new(vec![]),
                stop_after: Some(1),
            },
            work(),
        );
        let r = speak(
            "1 First. 2 Second. 3 Third. 4 Fourth. 5 Fifth.",
            Arc::new(SlowFirstSynth),
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(r.stopped);
        assert_eq!(r.sentences, 1);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn work() -> PathBuf {
        std::env::temp_dir().join(format!(
            "friday-speak-test-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ))
    }

    #[test]
    fn an_answer_is_spoken_in_order_a_sentence_at_a_time_and_cleaned_up() {
        let (synth, player, dir) = (
            Arc::new(FakeSynth {
                made: Mutex::new(vec![]),
                fail_on: None,
            }),
            FakePlayer {
                played: Mutex::new(vec![]),
                stop_after: None,
            },
            work(),
        );
        let r = speak(
            "First one. Second one! Third one?",
            synth.clone(),
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(
            r,
            Spoken {
                sentences: 3,
                stopped: false
            }
        );
        assert_eq!(
            *synth.made.lock().unwrap(),
            ["First one.", "Second one!", "Third one?"]
        );
        assert_eq!(
            *player.played.lock().unwrap(),
            ["part-0.wav", "part-1.wav", "part-2.wav"]
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            0,
            "no audio files are left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stopping_ends_the_rest_of_the_answer() {
        let (synth, player, dir) = (
            Arc::new(FakeSynth {
                made: Mutex::new(vec![]),
                fail_on: None,
            }),
            FakePlayer {
                played: Mutex::new(vec![]),
                stop_after: Some(1),
            },
            work(),
        );
        let r = speak(
            "One. Two. Three. Four. Five.",
            synth,
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(r.stopped);
        assert_eq!(r.sentences, 1);
        assert_eq!(player.played.lock().unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_engine_failure_is_an_error_only_when_nothing_was_said() {
        let player = FakePlayer {
            played: Mutex::new(vec![]),
            stop_after: None,
        };
        let dir = work();
        let early = speak(
            "Boom now. After.",
            Arc::new(FakeSynth {
                made: Mutex::new(vec![]),
                fail_on: Some("Boom"),
            }),
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(early.unwrap_err(), "engine broke");
        let late = speak(
            "Fine. Boom later.",
            Arc::new(FakeSynth {
                made: Mutex::new(vec![]),
                fail_on: Some("Boom"),
            }),
            &player,
            &dir,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(late.sentences, 1, "what was said stays said");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_is_accepted_only_when_it_matches_its_pin() {
        let dir = work();
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.bin");
        std::fs::write(&file, b"abc").unwrap();
        // sha256("abc")
        assert_eq!(
            sha256_hex(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_pins_are_full_sha256_digests_for_every_model() {
        for kind in [Kind::Supertonic, Kind::Kitten] {
            let (_, file, sha) = model_pin(kind);
            assert!(file.ends_with(".tar.bz2"));
            assert!(
                sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{file}"
            );
        }
        assert_eq!(ENGINE.2.len(), 64);
    }

    #[test]
    fn with_a_dump_folder_the_sentences_are_saved_not_played() {
        let (dir, out) = (work(), work());
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("part-0.wav");
        std::fs::write(&wav, write_wav(&tone(24000, 0.05, 0.3))).unwrap();
        SystemPlayer {
            dump: Some(out.clone()),
        }
        .play(&wav, &AtomicBool::new(false))
        .unwrap();
        let saved: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(saved.len(), 1);
        assert!(read_wav(&std::fs::read(saved[0].path()).unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn nothing_to_say_says_nothing() {
        let (synth, player, dir) = (
            Arc::new(FakeSynth {
                made: Mutex::new(vec![]),
                fail_on: None,
            }),
            FakePlayer {
                played: Mutex::new(vec![]),
                stop_after: None,
            },
            work(),
        );
        assert_eq!(
            speak(
                "```\ncode\n```",
                synth.clone(),
                &player,
                &dir,
                Arc::new(AtomicBool::new(false))
            )
            .unwrap()
            .sentences,
            1
        );
        assert_eq!(
            speak("  ", synth, &player, &dir, Arc::new(AtomicBool::new(false))).unwrap(),
            Spoken {
                sentences: 0,
                stopped: false
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_failed_reinstall_leaves_the_old_voice_working() {
        let root = std::env::temp_dir().join(format!("friday-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (new, dir) = (root.join("new"), root.join("voice"));
        let put = |d: &Path, name: &str, text: &str| {
            std::fs::create_dir_all(d.join(name)).unwrap();
            std::fs::write(d.join(name).join("f"), text).unwrap();
        };
        put(&dir, "bin", "old");
        put(&dir, "model", "old");
        // The new voice has a bin, and a part whose place in `dir` doesn't exist,
        // so its move fails after bin was already swapped in.
        put(&new, "bin", "new");
        std::fs::create_dir_all(new.join("nested/part")).unwrap();
        let err = swap_in(&new, &dir, &["bin", "nested/part", "model"]);
        assert!(err.is_err(), "the move into a missing folder should fail");
        let read = |p: &str| std::fs::read_to_string(dir.join(p).join("f")).unwrap();
        assert_eq!(read("bin"), "old", "the old engine must be back");
        assert_eq!(read("model"), "old", "the old model must be untouched");
        assert!(!dir.join(".old-bin").exists(), "nothing is left aside");

        // And a good install replaces everything and leaves nothing aside.
        let (new2, dir2) = (root.join("new2"), root.join("voice2"));
        put(&dir2, "bin", "old");
        put(&new2, "bin", "new");
        put(&new2, "model", "new");
        swap_in(&new2, &dir2, &VOICE_PARTS).unwrap();
        assert_eq!(std::fs::read_to_string(dir2.join("bin/f")).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(dir2.join("model/f")).unwrap(),
            "new"
        );
        assert!(!dir2.join(".old-bin").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_engine_failure_message_carries_its_own_last_line() {
        let msg = engine_failure(b"loading model\nError: model.onnx: no such file\n\n");
        assert!(msg.contains("model.onnx: no such file"), "{msg}");
        assert!(msg.contains("friday voice speak setup"), "{msg}");
        assert!(engine_failure(b"").contains("the voice engine failed"));
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_engine_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("friday-engine-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let engine = dir.join("bin").join("fake-engine");
        std::fs::write(
            &engine,
            "#!/bin/sh\necho 'cannot open model.onnx' >&2\nexit 3\n",
        )
        .unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let synth = EngineSynth {
            voice: Voice {
                engine,
                model: dir.join("model"),
                kind: Kind::Kitten,
                speaker: 0,
                speed: 1.0,
                threads: 1,
            },
            work_dir: dir.clone(),
        };
        let err = synth.synth("hello").expect_err("the engine fails");
        assert!(err.contains("cannot open model.onnx"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
