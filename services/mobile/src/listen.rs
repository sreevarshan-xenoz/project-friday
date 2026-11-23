//! Listening on this computer: live captions for any audio, and an opt-in
//! "hey Friday" wake word. Audio comes from PipeWire (`pw-record`), is cut
//! into spoken pieces by loudness, and each piece is turned into text by
//! whisper.cpp (the same model as `friday voice`). Audio is never stored or
//! sent anywhere; only the text of a piece leaves this module, and wake-word
//! listening hands on nothing unless a piece starts with "hey Friday".
//! On Windows, `hold` records the microphone while the talk key is held and
//! prints what was said (the tray asks it), with whisper.cpp that `setup`
//! downloads, pinned by sha256.

use std::path::Path;

/// Samples per second.
pub const RATE: usize = 16_000;
/// One frame is 100 ms.
pub const FRAME: usize = RATE / 10;

/// Cuts a stream of 100 ms frames into spoken pieces: a piece starts when a
/// frame is clearly louder than the room, keeps a little before it, and ends
/// after a pause or at its maximum length.
pub struct Segmenter {
    floor: f64,
    pre: std::collections::VecDeque<Vec<i16>>,
    piece: Vec<i16>,
    quiet: usize,
    speaking: bool,
    /// Frames of lead-in kept before the piece began.
    lead: usize,
    /// Frames heard while learning the room's noise level.
    heard: usize,
    /// The last piece was cut at its maximum length: what follows continues it.
    cut: bool,
    hang: usize,
    max_frames: usize,
}

const PRE_ROLL: usize = 3;
const CALIBRATE: usize = 10;
const MIN_LOUD: f64 = 250.0;
/// A piece shorter than this many frames is noise (a click, a cough).
const MIN_FRAMES: usize = 4;

pub fn rms(frame: &[i16]) -> f64 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / frame.len() as f64).sqrt()
}

impl Segmenter {
    /// `hang`: quiet frames that end a piece; `max_frames`: its longest.
    pub fn new(hang: usize, max_frames: usize) -> Self {
        Segmenter {
            floor: 100.0,
            pre: Default::default(),
            piece: Vec::new(),
            quiet: 0,
            speaking: false,
            lead: 0,
            heard: 0,
            cut: false,
            hang,
            max_frames,
        }
    }

    /// A segmenter that already knows this room (no calibration second).
    pub fn continuing(&self, hang: usize, max_frames: usize) -> Self {
        Segmenter {
            heard: CALIBRATE,
            floor: self.floor,
            ..Segmenter::new(hang, max_frames)
        }
    }

    /// Feeds one frame; returns a finished piece when one ends.
    pub fn feed(&mut self, frame: &[i16]) -> Option<Vec<i16>> {
        let level = rms(frame);
        // The first second only teaches the room's level, whatever it is.
        if self.heard < CALIBRATE {
            self.floor = (self.floor * self.heard as f64 + level) / (self.heard + 1) as f64;
            self.heard += 1;
            return None;
        }
        let loud = level > (self.floor * 3.0).max(MIN_LOUD);
        if !self.speaking {
            if !loud {
                // The room's own noise level, slowly followed.
                self.floor = 0.95 * self.floor + 0.05 * level;
                self.pre.push_back(frame.to_vec());
                if self.pre.len() > PRE_ROLL {
                    self.pre.pop_front();
                }
                return None;
            }
            self.speaking = true;
            self.quiet = 0;
            self.lead = self.pre.len();
            self.piece = self.pre.drain(..).flatten().collect();
        }
        self.piece.extend_from_slice(frame);
        self.quiet = if loud { 0 } else { self.quiet + 1 };
        let frames = self.piece.len() / FRAME;
        if self.quiet >= self.hang || frames >= self.max_frames {
            self.speaking = false;
            let piece = std::mem::take(&mut self.piece);
            let speech_frames = frames.saturating_sub(self.quiet + self.lead);
            // The end of a long piece, after a cut, may be short.
            let long_enough = speech_frames >= MIN_FRAMES || self.cut;
            self.cut = self.quiet < self.hang && frames >= self.max_frames;
            return long_enough.then_some(piece);
        }
        None
    }
}

/// 16-bit mono WAV bytes for whisper.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&(RATE as u32).to_le_bytes());
    out.extend_from_slice(&((RATE * 2) as u32).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Whisper's text without its bracketed markers ("[BLANK_AUDIO]", "(music)")
/// and extra spaces.
pub fn clean_text(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            '\n' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // On faint noise whisper makes up a lone "you".
    if text.eq_ignore_ascii_case("you") || text.eq_ignore_ascii_case("you.") {
        return String::new();
    }
    text
}

/// Sounds whisper writes out that nobody means to type or ask.
const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm", "hmm", "mm"];

/// The text without "um" and "uh": "So, um, open it" → "So, open it", and
/// a first word dropped hands its capital on.
pub fn drop_fillers(text: &str) -> String {
    let bare = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let kept: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !FILLERS.contains(&bare(w).as_str()))
        .collect();
    let mut out = kept.join(" ");
    if kept.first() != words.first() {
        if let Some(c) = out.chars().next() {
            out = c.to_uppercase().chain(out.chars().skip(1)).collect();
        }
    }
    out
}

/// The person's own words for voice (`dictionary` in FRIDAY's config folder):
/// a line `Kubernetes` is a word whisper should know (it is told them up
/// front), a line `cube cuddle => kubectl` fixes what it hears.
#[derive(Debug, Default, PartialEq)]
pub struct Dictionary {
    pub words: Vec<String>,
    pub fixes: Vec<(String, String)>,
}

impl Dictionary {
    pub fn parse(text: &str) -> Self {
        let mut dict = Dictionary::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match line.split_once("=>") {
                Some((heard, written))
                    if !heard.trim().is_empty() && !written.trim().is_empty() =>
                {
                    dict.fixes
                        .push((heard.trim().to_lowercase(), written.trim().to_string()))
                }
                Some(_) => {}
                None => dict.words.push(line.to_string()),
            }
        }
        dict
    }

    /// `dictionary` in FRIDAY's config folder (`%APPDATA%\friday` on Windows).
    pub fn load() -> Self {
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
        #[cfg(not(windows))]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
            });
        base.and_then(|b| std::fs::read_to_string(b.join("friday").join("dictionary")).ok())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// The text with each fix applied, whole words only, any case; a fixed
    /// phrase keeps the punctuation after it.
    pub fn apply(&self, text: &str) -> String {
        let mut words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
        for (heard, written) in &self.fixes {
            let want: Vec<&str> = heard.split_whitespace().collect();
            let mut i = 0;
            while !want.is_empty() && i + want.len() <= words.len() {
                let matches = want.iter().enumerate().all(|(k, w)| {
                    words[i + k]
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .eq_ignore_ascii_case(w)
                });
                if matches {
                    let last = &words[i + want.len() - 1];
                    let tail: String = last
                        .chars()
                        .rev()
                        .take_while(|c| !c.is_alphanumeric())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    words.splice(i..i + want.len(), [format!("{written}{tail}")]);
                }
                i += 1;
            }
        }
        words.join(" ")
    }
}

/// Whether the text starts with "hey Friday" (or "ok / okay / hi Friday"),
/// and what follows it. "Friday" alone isn't enough: it is a day of the week.
pub fn wake_phrase(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let bare = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    if words.len() < 2 || !matches!(bare(words[0]).as_str(), "hey" | "ok" | "okay" | "hi") {
        return None;
    }
    (bare(words[1]) == "friday").then(|| {
        words[2..]
            .join(" ")
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_string()
    })
}

/// What a heard piece means for the wake word and a hands-free conversation.
#[derive(Debug, PartialEq)]
pub enum Heard {
    /// "Hey Friday, keep listening": what follows needs no wake word.
    Start,
    /// "Stop listening" (or "that's all") during a conversation.
    Stop,
    /// A question (empty: the wake word alone, the question comes next).
    Ask(String),
    /// The wake word while FRIDAY speaks: stop speaking, then this question.
    Interrupt(String),
    Ignore,
}

/// Decides what `text` means. During a conversation no wake word is needed;
/// while FRIDAY speaks only the wake word counts (anything else is most
/// likely its own voice coming back through the microphone).
pub fn heard(text: &str, conversing: bool, speaking: bool) -> Heard {
    let plain = |t: &str| {
        t.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let stop_words = |t: &str| {
        matches!(
            t,
            "stop listening"
                | "thats all"
                | "that is all"
                | "goodbye"
                | "thank you thats all"
                | "thanks thats all"
        )
    };
    if let Some(rest) = wake_phrase(text) {
        if matches!(
            plain(&rest).as_str(),
            "keep listening" | "lets talk" | "let us talk" | "stay with me"
        ) {
            return Heard::Start;
        }
        // "Hey Friday, stop listening": ends the conversation (and the speech).
        if conversing && stop_words(&plain(&rest)) {
            return Heard::Stop;
        }
        return if speaking {
            Heard::Interrupt(rest)
        } else {
            Heard::Ask(rest)
        };
    }
    if !conversing || speaking {
        return Heard::Ignore;
    }
    let said = plain(text);
    match said.as_str() {
        "" => Heard::Ignore,
        "stop" => Heard::Stop,
        t if stop_words(t) => Heard::Stop,
        _ => Heard::Ask(text.trim().to_string()),
    }
}

/// Runs whisper.cpp on a piece and returns its cleaned text.
pub fn transcribe(whisper: &Path, model: &Path, samples: &[i16]) -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let file = dir.join(format!("friday-listen-{}.wav", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    std::io::Write::write_all(&mut opts.open(&file).ok()?, &wav_bytes(samples)).ok()?;
    #[cfg(unix)]
    let mut cmd = {
        let mut c = std::process::Command::new("timeout");
        c.arg("60").arg(whisper);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        // No console window flashing up from the tray.
        let mut c = std::process::Command::new(whisper);
        std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
        c
    };
    // Your own words go first, so whisper spells them your way.
    let cleanup = crate::features::on("dictation-cleanup");
    let dict = if cleanup {
        Dictionary::load()
    } else {
        Dictionary::default()
    };
    cmd.arg("-m")
        .arg(model)
        .arg("-f")
        .arg(&file)
        .args(["-nt", "-np"]);
    if !dict.words.is_empty() {
        cmd.arg("--prompt").arg(dict.words.join(", "));
    }
    let out = with_timeout(cmd.stderr(std::process::Stdio::null()));
    let _ = std::fs::remove_file(&file);
    let mut text = clean_text(&String::from_utf8_lossy(&out.ok()?.stdout));
    if cleanup {
        text = dict.apply(&drop_fillers(&text));
    }
    (!text.is_empty()).then_some(text)
}

/// Runs whisper.cpp and returns what it printed; `timeout 60` already bounds
/// it on Linux.
#[cfg(unix)]
fn with_timeout(cmd: &mut std::process::Command) -> std::io::Result<std::process::Output> {
    cmd.output()
}

/// Runs whisper.cpp and returns what it printed, stopping it after 60 s.
#[cfg(windows)]
fn with_timeout(cmd: &mut std::process::Command) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    let mut child = cmd.stdout(std::process::Stdio::piped()).spawn()?;
    let mut out = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() > until {
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let stdout = reader.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

/// whisper.cpp and the model `friday voice setup` installs, if both are there.
#[cfg(not(windows))]
pub fn find_whisper() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let path = std::env::var_os("PATH")?;
    let bin = std::env::split_paths(&path)
        .flat_map(|d| [d.join("whisper-cli"), d.join("whisper-cpp")])
        .find(|p| p.is_file())?;
    let model = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })?
        .join("friday/whisper/ggml-base.en.bin");
    model.is_file().then_some((bin, model))
}

/// whisper.cpp and the model `friday-ctl listen setup` installs, if both are there.
#[cfg(windows)]
pub fn find_whisper() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let dir = whisper_dir();
    let (bin, model) = (
        dir.join("Release").join("whisper-cli.exe"),
        dir.join(MODEL.0),
    );
    (bin.is_file() && model.is_file()).then_some((bin, model))
}

/// Where Windows keeps whisper.cpp and its model.
pub fn whisper_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("friday").join("whisper")
}

/// The English base model (MIT, ggerganov/whisper.cpp on Hugging Face, the
/// same one `friday voice setup` uses), pinned at one revision by sha256.
pub const MODEL: (&str, &str, &str) = (
    "ggml-base.en.bin",
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin",
    "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
);

/// whisper.cpp's own Windows build (MIT), pinned by the sha256 GitHub publishes.
pub const WHISPER_WIN: (&str, &str, &str) = (
    "whisper-bin-x64.zip",
    "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip",
    "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb",
);

/// Downloads whisper.cpp and the model for hold-to-talk (Windows; Linux has
/// `friday voice setup`). What is already there is kept.
pub fn setup(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !dir.join("Release").join("whisper-cli.exe").is_file() {
        let zip = dir.join(WHISPER_WIN.0);
        crate::speech::download(WHISPER_WIN.1, &zip, WHISPER_WIN.2)?;
        // Windows' own tar unpacks zip files too.
        let ok = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(dir)
            .status()
            .map_err(|e| format!("couldn't run tar: {e}"))?
            .success();
        let _ = std::fs::remove_file(&zip);
        if !ok {
            return Err("couldn't unpack whisper.cpp".into());
        }
    }
    let model = dir.join(MODEL.0);
    if !model.is_file() {
        let part = dir.join(format!("{}.part", MODEL.0));
        crate::speech::download(MODEL.1, &part, MODEL.2)?;
        std::fs::rename(&part, &model).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The longest hold that is recorded.
pub const HOLD_MAX_SECS: usize = 30;

/// Reads the microphone until `stop` says so (the key was let go) or `max`
/// samples are in, then once more for what came in meanwhile.
pub fn record_with(
    mut read: impl FnMut() -> Option<Vec<i16>>,
    stop: impl Fn() -> bool,
    max: usize,
) -> Vec<i16> {
    let mut all = Vec::new();
    // A microphone with nothing pending reads empty, so time counts too.
    let until = std::time::Instant::now()
        + std::time::Duration::from_secs_f64(max as f64 / RATE as f64 + 1.0);
    while !stop() && all.len() < max && std::time::Instant::now() < until {
        match read() {
            Some(piece) => all.extend(piece),
            None => break,
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if let Some(piece) = read() {
        all.extend(piece);
    }
    all.truncate(max);
    all
}

/// Hold-to-talk: records until stdin closes (the tray closes it when the key
/// is let go), then prints what was said. Exit 0 with the text; 1 when
/// nothing was caught; 3 when voice isn't set up; 4 with no microphone; 5
/// when the key was let go almost at once (a tap: the tray opens the box).
pub fn run_hold() -> u8 {
    let Some((whisper, model)) = find_whisper() else {
        eprintln!("friday-ctl: voice isn't set up yet: friday-ctl listen setup");
        return 3;
    };
    let Some(mic) = friday_sensors::audio::Mic::open() else {
        eprintln!("friday-ctl: couldn't open the microphone");
        return 4;
    };
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        stopped.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let samples = record_with(
        || mic.read(),
        || stop.load(std::sync::atomic::Ordering::SeqCst),
        RATE * HOLD_MAX_SECS,
    );
    // Under a quarter of a second is a tap of the key, not speech.
    if samples.len() < RATE / 4 {
        return 5;
    }
    match transcribe(&whisper, &model, &samples) {
        Some(text) => {
            println!("{text}");
            0
        }
        None => 1,
    }
}

/// How long a spoken answer may take to begin after the chime.
pub const ANSWER_WAIT_FRAMES: usize = 60;

/// One short spoken answer, for Run/Cancel by voice: the first second learns
/// the room ("listening" is printed then, for the caller's chime), then the
/// first thing said within six seconds is printed. Exit 0 with the text; 1
/// when nothing was said; 3 when voice isn't set up; 4 with no microphone.
pub fn run_answer() -> u8 {
    use std::io::Write;
    let Some((whisper, model)) = find_whisper() else {
        eprintln!("friday-ctl: voice isn't set up yet");
        return 3;
    };
    let Some(frames) = mic_frames() else {
        eprintln!("friday-ctl: couldn't open the microphone");
        return 4;
    };
    let mut seg = Segmenter::new(6, 40);
    for (n, frame) in frames.enumerate() {
        if n >= CALIBRATE + ANSWER_WAIT_FRAMES {
            break;
        }
        let piece = seg.feed(&frame);
        if n + 1 == CALIBRATE {
            println!("listening");
            let _ = std::io::stdout().flush();
        }
        if let Some(text) = piece.and_then(|p| transcribe(&whisper, &model, &p)) {
            println!("{text}");
            return 0;
        }
    }
    1
}

/// The microphone as 100 ms frames: PipeWire on Linux, WASAPI on Windows.
fn mic_frames() -> Option<Box<dyn Iterator<Item = Vec<i16>>>> {
    #[cfg(windows)]
    {
        let mic = friday_sensors::audio::Mic::open()?;
        let mut buf: Vec<i16> = Vec::new();
        Some(Box::new(std::iter::from_fn(move || loop {
            if buf.len() >= FRAME {
                return Some(buf.drain(..FRAME).collect());
            }
            buf.extend(mic.read()?);
            std::thread::sleep(std::time::Duration::from_millis(20));
        })))
    }
    #[cfg(not(windows))]
    {
        // With talk-over on, the echo-cancelled microphone: FRIDAY's own voice
        // is taken out of it.
        frames_from(talk_over_on().then_some(ECHO_SOURCE))
            .ok()
            .map(|f| Box::new(f) as Box<dyn Iterator<Item = Vec<i16>>>)
    }
}

/// Talk-over (`friday voice talk-over on`, Linux): PipeWire's echo
/// cancellation gives FRIDAY its own speaker and microphone, so what it says
/// is taken out of what the microphone hears and the person can talk over it.
pub const ECHO_SINK: &str = "friday-echo-sink";
pub const ECHO_SOURCE: &str = "friday-echo-source";

/// The PipeWire setting `friday voice talk-over on` writes.
pub fn talk_over_conf() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("pipewire/pipewire.conf.d/friday-echo-cancel.conf"))
}

/// On, and FRIDAY can play through its echo-cancelled speaker (`pw-play`):
/// without that, the microphone would hear FRIDAY's own voice.
pub fn talk_over_on() -> bool {
    cfg!(not(windows))
        && talk_over_conf().is_some_and(|p| p.is_file())
        && std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("pw-play").is_file()))
}

/// Loud frames in a row (0.4 s) before FRIDAY stops talking for them: a
/// click or a cough doesn't.
pub const BARGE_FRAMES: usize = 4;

/// Talking over FRIDAY (talk-over on): while it speaks, listens on the
/// echo-cancelled microphone; once the person has spoken for a moment, the
/// speech stops and what they say is printed. Exit 0 with the text; 1 when
/// nothing was said before the speech ended; 3 without voice or talk-over.
pub fn run_barge() -> u8 {
    let Some((whisper, model)) = find_whisper() else {
        return 3;
    };
    if !talk_over_on() {
        return 3;
    }
    let Ok(frames) = frames_from(Some(ECHO_SOURCE)) else {
        return 3;
    };
    let mut seg = Segmenter::new(8, 150);
    let (mut loud, mut started, mut stopped, mut n) = (0, false, false, 0usize);
    for frame in frames {
        n += 1;
        if !stopped {
            // The reading may not have begun yet; once it has, its end ends this.
            if crate::speech::speaking() {
                started = true;
            } else if started || n > 30 {
                return 1;
            }
        } else if n > 80 {
            // Stopped for something that never became words.
            return 1;
        }
        let level = rms(&frame);
        let piece = seg.feed(&frame);
        if !stopped {
            let clear = seg.heard >= CALIBRATE && level > (seg.floor * 3.0).max(MIN_LOUD);
            loud = if clear { loud + 1 } else { 0 };
            if loud >= BARGE_FRAMES {
                crate::speech::stop_running();
                stopped = true;
                n = 0;
            }
        }
        if let Some(p) = piece {
            if !stopped {
                continue;
            }
            return match transcribe(&whisper, &model, &p) {
                Some(text) => {
                    println!("{text}");
                    0
                }
                None => 1,
            };
        }
    }
    1
}

/// Frames of 100 ms from PipeWire: what is heard (`sink`: the sound the
/// computer plays) or the microphone. Ends when the recorder does.
fn frames(sink: bool) -> std::io::Result<impl Iterator<Item = Vec<i16>>> {
    let mut cmd = recorder();
    if sink {
        cmd.args(["-P", "stream.capture.sink=true"]);
    }
    read_frames(cmd)
}

/// Frames of 100 ms from one PipeWire source (`None`: the default microphone).
fn frames_from(target: Option<&str>) -> std::io::Result<impl Iterator<Item = Vec<i16>>> {
    let mut cmd = recorder();
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    read_frames(cmd)
}

fn recorder() -> std::process::Command {
    let mut cmd = std::process::Command::new("pw-record");
    cmd.args([
        "--raw",
        "--rate",
        "16000",
        "--channels",
        "1",
        "--format",
        "s16",
    ]);
    cmd
}

fn read_frames(mut cmd: std::process::Command) -> std::io::Result<impl Iterator<Item = Vec<i16>>> {
    use std::io::Read;
    let mut child = cmd
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
    Ok(std::iter::from_fn(move || {
        let mut buf = vec![0u8; FRAME * 2];
        if out.read_exact(&mut buf).is_err() {
            let _ = child.kill();
            return None;
        }
        Some(
            buf.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect(),
        )
    }))
}

fn friday(args: &[&str]) {
    let _ = std::process::Command::new("friday")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Live captions: every spoken piece of what the computer plays (or, with
/// `mic`, the microphone) is shown on screen and kept in a notes file.
/// Runs until stopped.
pub fn run_captions(mic: bool) -> u8 {
    use std::io::Write;
    let Some((whisper, model)) = find_whisper() else {
        eprintln!("friday-ctl: captions need voice first: friday voice setup");
        return 1;
    };
    let Ok(frames) = frames(!mic) else {
        eprintln!("friday-ctl: couldn't start pw-record");
        return 1;
    };
    let notes = notes_file(chrono::Local::now());
    if let Some(dir) = notes.parent() {
        let _ = std::fs::create_dir_all(dir);
        // What was said can be private: only you can open the notes.
        #[cfg(unix)]
        let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    println!("{}", notes.display());
    let mut seg = Segmenter::new(6, 80);
    let mut last = String::new();
    let mut shown: Vec<String> = Vec::new();
    for frame in frames {
        let Some(piece) = seg.feed(&frame) else {
            continue;
        };
        let Some(text) = transcribe(&whisper, &model, &piece) else {
            continue;
        };
        if text == last {
            continue;
        }
        last = text.clone();
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        if let Ok(mut f) = opts.open(&notes) {
            let _ = writeln!(f, "{} {text}", chrono::Local::now().format("%H:%M:%S"));
        }
        shown.push(text);
        if shown.len() > 2 {
            shown.remove(0);
        }
        friday(&["shell", "ipc", "captions", "put", &shown.join("\n")]);
    }
    0
}

/// The wake word: listens to the microphone for a piece that starts with
/// "hey Friday", then asks what follows it (or, when nothing does, listens
/// for the question). Nothing is kept or sent until then.
pub fn run_wake() -> u8 {
    let Some((whisper, model)) = find_whisper() else {
        eprintln!("friday-ctl: the wake word needs voice first: friday voice setup");
        return 1;
    };
    let Some(mut frames) = mic_frames() else {
        eprintln!("friday-ctl: couldn't open the microphone");
        return 1;
    };
    let mut seg = Segmenter::new(8, 50);
    let mut quiet_until = std::time::Instant::now();
    // A hands-free conversation: on since this moment, ends after a minute of quiet.
    let mut conversing: Option<std::time::Instant> = None;
    // When an answer was last heard being spoken (its echo is ignored).
    let mut last_spoke = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(60))
        .unwrap_or_else(std::time::Instant::now);
    let note = |text: &str| tell("note", text);
    while let Some(frame) = frames.next() {
        if conversing.is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(60))
            && !crate::speech::speaking()
        {
            conversing = None;
            note("Stopped listening.");
        }
        if conversing.is_some() && crate::speech::speaking() {
            last_spoke = std::time::Instant::now();
        }
        let Some(piece) = seg.feed(&frame) else {
            continue;
        };
        if std::time::Instant::now() < quiet_until {
            continue;
        }
        let Some(text) = transcribe(&whisper, &model, &piece) else {
            continue;
        };
        // Its own voice can still be in the piece just after it stopped speaking.
        let echo = crate::speech::speaking()
            || last_spoke.elapsed() < std::time::Duration::from_millis(1500);
        let (rest, follow_up) = match heard(&text, conversing.is_some(), echo) {
            Heard::Ignore => continue,
            Heard::Start => {
                conversing = Some(std::time::Instant::now());
                note("Listening: just talk. Say \"stop listening\" to end.");
                seg = Segmenter::new(8, 50);
                continue;
            }
            Heard::Stop => {
                crate::speech::stop_running();
                conversing = None;
                note("Stopped listening.");
                continue;
            }
            Heard::Interrupt(rest) => {
                crate::speech::stop_running();
                // "Hey Friday, stop" only stops the speech.
                if matches!(
                    rest.trim()
                        .to_lowercase()
                        .trim_matches(|c: char| !c.is_alphanumeric()),
                    "stop" | "quiet" | "be quiet" | "enough"
                ) {
                    continue;
                }
                (rest, conversing.is_some())
            }
            Heard::Ask(rest) => (rest, conversing.is_some()),
        };
        if conversing.is_some() {
            conversing = Some(std::time::Instant::now());
        }
        let mut question = rest;
        if question.trim().is_empty() {
            note("Listening…");
            // The question: the next piece, if it starts within 8 seconds.
            let mut ask = seg.continuing(12, 150);
            let mut waited = 0;
            question = String::new();
            for f in frames.by_ref() {
                waited += 1;
                if let Some(p) = ask.feed(&f) {
                    question = transcribe(&whisper, &model, &p).unwrap_or_default();
                    break;
                }
                if waited > 80 + 150 {
                    break;
                }
            }
        }
        if !question.trim().is_empty() {
            // On its own thread: the answer streams for a while, and the
            // microphone's pipe mustn't back up meanwhile. In a conversation
            // each question follows on from the last answer.
            std::thread::spawn(move || tell(if follow_up { "followup" } else { "ask" }, &question));
        }
        quiet_until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        seg = Segmenter::new(8, 50);
    }
    0
}

/// What the wake word heard, handed on: on Linux a notification or `friday
/// ask` (`--follow-up` in a conversation); on Windows a line `<kind>\t<text>`
/// for the tray, which asks it.
fn tell(kind: &str, text: &str) {
    #[cfg(windows)]
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{kind}\t{}", text.replace(['\n', '\t'], " "));
        let _ = out.flush();
    }
    #[cfg(not(windows))]
    match kind {
        "note" => {
            let _ = std::process::Command::new("notify-send")
                .args(["-a", "FRIDAY", "-t", "4000", "--", "FRIDAY", text])
                .stderr(std::process::Stdio::null())
                .status();
        }
        "followup" => friday(&["ask", "--follow-up", text]),
        _ => friday(&["ask", text]),
    }
}

/// Where captions' notes are kept: `captions/<date>-<time>.txt` under
/// FRIDAY's data directory.
pub fn notes_file(now: chrono::DateTime<chrono::Local>) -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| "/tmp".into())
        .join("friday/captions")
        .join(format!("{}.txt", now.format("%Y-%m-%d-%H%M")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillers_go_and_the_sentence_stays() {
        assert_eq!(drop_fillers("So, um, open Firefox"), "So, open Firefox");
        assert_eq!(
            drop_fillers("um open firefox uh please"),
            "Open firefox please"
        );
        assert_eq!(drop_fillers("Umbrella or summary"), "Umbrella or summary");
        assert_eq!(drop_fillers("Uh."), "");
    }

    #[test]
    fn the_dictionary_teaches_words_and_fixes_what_was_heard() {
        let dict = Dictionary::parse(
            "# my words\nKubernetes\nSreevarsh => Sreevarshan\ncube cuddle => kubectl\n=> nothing\n",
        );
        assert_eq!(dict.words, vec!["Kubernetes".to_string()]);
        assert_eq!(dict.fixes.len(), 2);
        assert_eq!(
            dict.apply("Run cube cuddle get pods, then CUBE CUDDLE logs."),
            "Run kubectl get pods, then kubectl logs."
        );
        assert_eq!(dict.apply("cubes cuddle"), "cubes cuddle");
    }

    fn tone(level: i16) -> Vec<i16> {
        (0..FRAME)
            .map(|i| if i % 2 == 0 { level } else { -level })
            .collect()
    }

    #[test]
    fn a_conversation_starts_stops_and_needs_no_wake_word_in_between() {
        assert_eq!(
            heard("Hey Friday, keep listening.", false, false),
            Heard::Start
        );
        assert_eq!(heard("Okay Friday let's talk", false, false), Heard::Start);
        assert_eq!(
            heard("what time is it in Tokyo", false, false),
            Heard::Ignore
        );
        assert_eq!(
            heard("What time is it in Tokyo?", true, false),
            Heard::Ask("What time is it in Tokyo?".into())
        );
        assert_eq!(heard("Stop listening.", true, false), Heard::Stop);
        assert_eq!(heard("Thanks, that's all!", true, false), Heard::Stop);
        assert_eq!(
            heard("Hey Friday, stop listening", true, false),
            Heard::Stop
        );
        assert_eq!(heard("Hey Friday, stop listening", true, true), Heard::Stop);
        assert_eq!(
            heard("hey friday what is rust", false, false),
            Heard::Ask("what is rust".into())
        );
    }

    #[test]
    fn while_friday_speaks_only_the_wake_word_counts() {
        // Its own voice through the microphone isn't a question.
        assert_eq!(
            heard("The capital of Peru is Lima.", true, true),
            Heard::Ignore
        );
        assert_eq!(
            heard("Hey Friday, stop", true, true),
            Heard::Interrupt("stop".into())
        );
        assert_eq!(
            heard("Hey Friday", false, true),
            Heard::Interrupt(String::new())
        );
    }

    #[test]
    fn a_spoken_piece_is_cut_out_with_a_little_before_it() {
        let mut s = Segmenter::new(5, 50);
        let mut got = Vec::new();
        // A second of calibration, then the room.
        for _ in 0..14 {
            assert!(s.feed(&tone(20)).is_none());
        }
        for _ in 0..8 {
            assert!(s.feed(&tone(4000)).is_none());
        }
        for _ in 0..5 {
            got.extend(s.feed(&tone(20)));
        }
        assert_eq!(got.len(), 1);
        // 3 frames before, 8 spoken, 5 of pause.
        assert_eq!(got[0].len() / FRAME, 16);
    }

    #[test]
    fn a_click_is_not_a_piece_and_a_long_one_is_cut() {
        let mut s = Segmenter::new(3, 10);
        for _ in 0..12 {
            s.feed(&tone(20));
        }
        s.feed(&tone(5000));
        let click = (0..3).filter_map(|_| s.feed(&tone(20))).count();
        assert_eq!(click, 0);
        let mut pieces = 0;
        for _ in 0..25 {
            pieces += s.feed(&tone(5000)).is_some() as usize;
        }
        assert!(pieces >= 2, "{pieces}");
    }

    #[test]
    fn a_continuing_segmenter_hears_at_once_and_a_cut_tail_may_be_short() {
        let mut first = Segmenter::new(3, 10);
        for _ in 0..14 {
            first.feed(&tone(20));
        }
        // No second of calibration: speech right away is heard.
        let mut next = first.continuing(3, 10);
        let mut pieces = 0;
        for _ in 0..6 {
            pieces += next.feed(&tone(4000)).is_some() as usize;
        }
        for _ in 0..4 {
            pieces += next.feed(&tone(20)).is_some() as usize;
        }
        assert_eq!(pieces, 1);
        // A long piece is cut at 10 frames; two more spoken frames after the
        // cut still make a piece.
        let mut long = first.continuing(3, 10);
        let mut got = 0;
        for _ in 0..10 {
            got += long.feed(&tone(4000)).is_some() as usize;
        }
        for _ in 0..2 {
            got += long.feed(&tone(4000)).is_some() as usize;
        }
        for _ in 0..3 {
            got += long.feed(&tone(20)).is_some() as usize;
        }
        assert_eq!(got, 2);
    }

    #[test]
    fn a_noisy_room_raises_the_bar() {
        let mut s = Segmenter::new(3, 50);
        for _ in 0..40 {
            assert!(s.feed(&tone(600)).is_none());
        }
        // The same level is the room, not speech.
        assert!(s.feed(&tone(700)).is_none() && !s.speaking);
    }

    #[test]
    fn the_wav_has_a_proper_header() {
        let w = wav_bytes(&[1, -1, 2]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(
            u32::from_le_bytes(w[24..28].try_into().unwrap()),
            RATE as u32
        );
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
        assert_eq!(w.len(), 50);
    }

    #[test]
    fn whispers_markers_are_dropped() {
        assert_eq!(clean_text(" [BLANK_AUDIO]\n"), "");
        assert_eq!(
            clean_text("Hello (music) there\n  friend [x] "),
            "Hello there friend"
        );
        assert_eq!(clean_text(" you\n"), "");
        assert_eq!(clean_text("Thank you"), "Thank you");
    }

    #[test]
    fn only_hey_friday_wakes_it() {
        assert_eq!(
            wake_phrase("Hey Friday, what time is it?").as_deref(),
            Some("what time is it?")
        );
        assert_eq!(wake_phrase("okay friday").as_deref(), Some(""));
        assert_eq!(
            wake_phrase("Hey, Friday! open the settings").as_deref(),
            Some("open the settings")
        );
        assert_eq!(wake_phrase("see you on Friday"), None);
        assert_eq!(wake_phrase("Friday is the day"), None);
        assert_eq!(wake_phrase("hey there friday"), None);
        assert_eq!(wake_phrase(""), None);
    }

    #[test]
    fn notes_are_named_by_time() {
        let t = chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2026, 11, 20, 9, 5, 0).unwrap();
        assert!(notes_file(t).ends_with("friday/captions/2026-11-20-0905.txt"));
    }

    #[test]
    fn a_hold_records_until_the_key_is_let_go() {
        let reads = std::cell::Cell::new(0);
        let got = record_with(
            || {
                reads.set(reads.get() + 1);
                Some(vec![1; 100])
            },
            || reads.get() >= 3,
            10_000,
        );
        // Three reads while held, one more for what came in meanwhile.
        assert_eq!(got.len(), 400);
    }

    #[test]
    fn a_hold_stops_at_its_longest() {
        let got = record_with(|| Some(vec![1; 300]), || false, 1_000);
        assert_eq!(got.len(), 1_000);
    }

    #[test]
    fn a_microphone_that_goes_away_ends_the_hold() {
        let n = std::cell::Cell::new(0);
        let got = record_with(
            || {
                n.set(n.get() + 1);
                (n.get() < 3).then(|| vec![1; 10])
            },
            || false,
            10_000,
        );
        assert_eq!(got.len(), 20);
    }

    #[test]
    fn the_downloads_are_pinned_by_full_digests() {
        for (_, url, sha) in [MODEL, WHISPER_WIN] {
            assert!(url.starts_with("https://"));
            assert_eq!(sha.len(), 64);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
        // The model at a fixed revision, not whatever "main" is today.
        assert!(!MODEL.1.contains("/main/"));
    }
}
