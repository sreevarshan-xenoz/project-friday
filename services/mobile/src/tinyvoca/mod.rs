//! tiny-voca text to speech, in process: a Rust port of its phonemizer (dizione,
//! Apache-2.0) and the ONNX model run through ONNX Runtime.

mod align;
mod convert;
mod engine;
mod g2p;
mod homographs;
mod morphology;
mod normalize;
mod numbers;
mod sentence;
mod symbols;
mod tables;
mod voice;

pub use engine::Phonemizer;
pub use symbols::Phonemized;
pub use voice::{Voca, SAMPLE_RATE, VOICES};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The model folder (`model.onnx` and `phonemizer/`, as tiny-voca ships them);
    /// the tests that need it are skipped when `FRIDAY_TINYVOCA_DATA` is not set.
    fn data() -> Option<PathBuf> {
        std::env::var_os("FRIDAY_TINYVOCA_DATA").map(PathBuf::from)
    }

    /// `testdata/gold.json` holds what dizione's Python code makes of each text
    /// (its own tricky sentences and some FRIDAY-style ones); the port must make
    /// the same symbols and stress.
    #[test]
    fn phonemes_match_the_python_original() {
        let Some(dir) = data() else { return };
        let gold: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("testdata/gold.json")).unwrap();
        assert!(gold.len() > 200, "the gold file is missing sentences");
        let phonemizer = Phonemizer::new(&dir.join("phonemizer")).unwrap();
        let mut wrong = Vec::new();
        for g in &gold {
            let text = g["text"].as_str().unwrap();
            let want_symbols: Vec<String> = g["symbols"]
                .as_array()
                .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
                .unwrap_or_default();
            let want_stress: Vec<i64> = g["stress"]
                .as_array()
                .map(|a| a.iter().map(|s| s.as_i64().unwrap()).collect())
                .unwrap_or_default();
            match phonemizer.phonemize(text) {
                Some(got) if got.symbols == want_symbols && got.stress == want_stress => {}
                Some(got) => wrong.push(format!(
                    "{text}\n   want {}\n   got  {}",
                    want_symbols.join(" "),
                    got.symbols.join(" ")
                )),
                None => wrong.push(format!("{text}\n   nothing")),
            }
        }
        eprintln!("{} sentences compared", gold.len());
        assert!(
            wrong.is_empty(),
            "{} of {} differ:\n{}",
            wrong.len(),
            gold.len(),
            wrong
                .iter()
                .take(15)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Needs `FRIDAY_TINYVOCA_RUNTIME` too: the ONNX Runtime library to load.
    #[test]
    fn speaks_a_sentence_with_the_model() {
        let (Some(dir), Some(runtime)) = (data(), std::env::var_os("FRIDAY_TINYVOCA_RUNTIME"))
        else {
            return;
        };
        let voca = Voca::load(&dir, std::path::Path::new(&runtime), 2).unwrap();
        for voice in 0..VOICES.len() {
            let samples = voca.speak("Hello! This is tiny voca.", voice, 1.0).unwrap();
            let seconds = samples.len() as f32 / 24_000.0;
            assert!((1.0..8.0).contains(&seconds), "{seconds} s of speech");
            assert!(
                samples.iter().any(|s| s.unsigned_abs() > 3000),
                "the audio is silent"
            );
            if let Some(out) = std::env::var_os("FRIDAY_TINYVOCA_WAV") {
                let wav = crate::speech::write_wav(&crate::speech::Pcm {
                    rate: 24_000,
                    samples,
                });
                std::fs::write(
                    std::path::Path::new(&out).join(format!("voice-{voice}.wav")),
                    wav,
                )
                .unwrap();
            }
        }
        assert!(voca.speak("   ", 0, 1.0).unwrap().is_empty());
    }
}
