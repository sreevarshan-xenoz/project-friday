//! tiny-voca speaking: text -> phonemes (this module's phonemizer) -> the ONNX
//! model through ONNX Runtime, loaded from the library that ships with the
//! sherpa-onnx engine package, so nothing else has to be installed.

use super::engine::Phonemizer;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

pub const SAMPLE_RATE: u32 = 24_000;
/// The model reads at most this many tokens per call; longer text is split.
const MAX_TOKENS: usize = 500;
/// The model can peak above 1.0; audio is scaled down to this.
const PEAK: f32 = 0.99;
/// The voices of the model, in the order its `voice` input counts them.
pub const VOICES: [&str; 2] = ["Female", "Male"];

struct Inner {
    phonemizer: Phonemizer,
    session: Session,
}

pub struct Voca {
    inner: Mutex<Inner>,
}

impl Voca {
    /// `model` holds `model.onnx` and `phonemizer/`; `runtime` is the ONNX Runtime library.
    pub fn load(model: &Path, runtime: &Path, threads: usize) -> Result<Self, String> {
        let phonemizer = Phonemizer::new(&model.join("phonemizer"))?;
        let env = ort::init_from(runtime)
            .map_err(|e| format!("couldn't load ONNX Runtime from {}: {e}", runtime.display()))?
            .with_name("friday")
            .build()
            .map_err(|e| e.to_string())?;
        let load = |e: &dyn std::fmt::Display| format!("couldn't load the voice model: {e}");
        let session = Session::builder(&env)
            .map_err(|e| load(&e))?
            .with_intra_threads(threads.max(1))
            .map_err(|e| load(&e))?
            .commit_from_file(model.join("model.onnx"))
            .map_err(|e| load(&e))?;
        Ok(Self {
            inner: Mutex::new(Inner {
                phonemizer,
                session,
            }),
        })
    }

    /// Speech for `text` as 16-bit samples at 24 kHz. `voice` counts `VOICES`;
    /// `speed` 1.0 is the model's own pace. Text with nothing to read gives no samples.
    pub fn speak(&self, text: &str, voice: usize, speed: f32) -> Result<Vec<i16>, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "the voice is broken".to_string())?;
        let mut audio = Vec::new();
        for part in split(&inner.phonemizer, text) {
            audio.extend(utterance(&mut inner, &part, voice, speed)?);
        }
        let peak = audio.iter().fold(0.0f32, |m, a| m.max(a.abs()));
        let scale = if peak > PEAK { PEAK / peak } else { 1.0 };
        Ok(audio
            .iter()
            .map(|a| (a * scale * 32767.0).round().clamp(-32768.0, 32767.0) as i16)
            .collect())
    }
}

/// The text in pieces the model can read in one call: the whole text when it
/// fits, else cut near the middle at a space, again and again.
fn split(phonemizer: &Phonemizer, text: &str) -> Vec<String> {
    let too_long = phonemizer
        .phonemize(text)
        .is_some_and(|p| p.ids.len() > MAX_TOKENS);
    let chars: Vec<char> = text.chars().collect();
    if !too_long || chars.len() < 2 {
        return vec![text.to_string()];
    }
    let mid = chars.len() / 2;
    let cut = (0..mid)
        .flat_map(|d| [mid + d, mid - d])
        .find(|&i| i > 0 && i < chars.len() && chars[i] == ' ')
        .unwrap_or(mid);
    let (a, b): (String, String) = (chars[..cut].iter().collect(), chars[cut..].iter().collect());
    let mut out = split(phonemizer, a.trim());
    out.extend(split(phonemizer, b.trim()));
    out
}

fn utterance(inner: &mut Inner, text: &str, voice: usize, speed: f32) -> Result<Vec<f32>, String> {
    let Some(mut phones) = inner.phonemizer.phonemize(text) else {
        return Ok(Vec::new());
    };
    // Text that ends without a mark gets a full stop: without one the last word breaks off.
    let last = phones
        .symbols
        .len()
        .checked_sub(2)
        .map(|i| phones.symbols[i].as_str())
        .unwrap_or("");
    if !matches!(last, "." | "?" | "!" | "…" | "," | ";" | ":" | "—") {
        if let Some(p) = inner.phonemizer.phonemize(&format!("{}.", text.trim_end())) {
            phones = p;
        }
    }
    let n = phones.ids.len();
    let out = inner
        .session
        .run(ort::inputs![
            "tokens" => Tensor::from_array(([1usize, n], phones.ids)).map_err(|e| e.to_string())?,
            "stress" => Tensor::from_array(([1usize, n], phones.stress)).map_err(|e| e.to_string())?,
            "voice" => Tensor::from_array(([1usize], vec![voice.min(VOICES.len() - 1) as i64])).map_err(|e| e.to_string())?,
            "speed" => Tensor::from_array(([1usize], vec![speed])).map_err(|e| e.to_string())?,
            "pitch_shift" => Tensor::from_array(([1usize], vec![0.0f32])).map_err(|e| e.to_string())?,
            "pitch_range" => Tensor::from_array(([1usize], vec![1.0f32])).map_err(|e| e.to_string())?,
        ])
        .map_err(|e| format!("the voice model failed: {e}"))?;
    let (_, data) = out["audio"]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    Ok(data.to_vec())
}
