//! The phone inventory and the token sequence built from it (a port of
//! dizione's `symbols.py`, Apache-2.0). The order is append-only: the model was
//! trained on these ids.

use std::collections::HashMap;
use std::sync::OnceLock;

pub type Phone = (String, u8);

pub const PAD: &str = "<pad>";
pub const BOS: &str = "<bos>";
pub const EOS: &str = "<eos>";
pub const SP: &str = "<sp>";
pub const UNK: &str = "<unk>";
const SPECIALS: [&str; 5] = [PAD, BOS, EOS, SP, UNK];
pub const PUNCTUATION: [&str; 12] = [",", ".", "?", "!", ";", ":", "\"", "'", "(", ")", "—", "…"];
const CONSONANTS: [&str; 26] = [
    "p", "b", "t", "d", "k", "ɡ", "f", "v", "θ", "ð", "s", "z", "ʃ", "ʒ", "h", "m", "n", "ŋ", "l",
    "ɹ", "j", "w", "tʃ", "dʒ", "ɾ", "ʔ",
];
const VOWELS: [&str; 33] = [
    "ə", "ɐ", "ɚ", "ᵻ", "ɪ", "i", "iː", "ɛ", "æ", "ʌ", "ɑː", "ɔ", "ɔː", "ʊ", "uː", "ɜː", "eɪ",
    "aɪ", "ɔɪ", "oʊ", "aʊ", "iə", "oː", "ɑːɹ", "ɔːɹ", "oːɹ", "ɪɹ", "ɛɹ", "ʊɹ", "aɪɚ", "aɪə", "əl",
    "n̩",
];

/// Every symbol in id order.
pub fn symbols() -> &'static Vec<&'static str> {
    static ALL: OnceLock<Vec<&'static str>> = OnceLock::new();
    ALL.get_or_init(|| {
        SPECIALS
            .iter()
            .chain(PUNCTUATION.iter())
            .chain(CONSONANTS.iter())
            .chain(VOWELS.iter())
            .copied()
            .collect()
    })
}

fn ids() -> &'static HashMap<&'static str, i64> {
    static IDS: OnceLock<HashMap<&'static str, i64>> = OnceLock::new();
    IDS.get_or_init(|| {
        symbols()
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i as i64))
            .collect()
    })
}

pub fn is_phone(s: &str) -> bool {
    CONSONANTS.contains(&s) || VOWELS.contains(&s)
}

pub fn is_vowel(s: &str) -> bool {
    VOWELS.contains(&s)
}

pub fn stress_mark(level: u8) -> &'static str {
    match level {
        1 => "ˈ",
        2 => "ˌ",
        _ => "",
    }
}

/// What the model takes: ids and stress levels, plus the symbols for reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phonemized {
    pub symbols: Vec<String>,
    pub ids: Vec<i64>,
    pub stress: Vec<i64>,
}

impl Phonemized {
    /// "h ə l ˈoʊ", the way dizione prints it.
    pub fn text(&self) -> String {
        self.symbols
            .iter()
            .zip(&self.stress)
            .map(|(s, l)| format!("{}{}", stress_mark(*l as u8), s))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A run of the clause stream: punctuation, or words (each a list of phones).
#[derive(Debug, Clone)]
pub enum Piece {
    Punct(String),
    Words(Vec<Vec<Phone>>),
}

/// The token sequence from the pieces: words are separated by `<sp>`, repeated
/// punctuation is collapsed. `None` when there is nothing to pronounce.
pub fn tokenize(pieces: &[Piece]) -> Option<Phonemized> {
    let (mut syms, mut stress): (Vec<String>, Vec<i64>) = (vec![BOS.into()], vec![0]);
    for piece in pieces {
        match piece {
            Piece::Punct(mark) => {
                if syms.last().map(String::as_str) != Some(mark.as_str()) {
                    syms.push(mark.clone());
                    stress.push(0);
                }
            }
            Piece::Words(words) => {
                for word in words {
                    let last = syms.last().map(String::as_str).unwrap_or("");
                    if is_phone(last) || last == UNK {
                        syms.push(SP.into());
                        stress.push(0);
                    }
                    for (phone, level) in word {
                        if is_phone(phone) {
                            syms.push(phone.clone());
                            stress.push(*level as i64);
                        } else {
                            syms.push(UNK.into());
                            stress.push(0);
                        }
                    }
                }
            }
        }
    }
    syms.push(EOS.into());
    stress.push(0);
    if !syms.iter().any(|s| is_phone(s) || s == UNK) {
        return None;
    }
    let ids_of = syms.iter().map(|s| ids()[s.as_str()]).collect();
    Some(Phonemized {
        symbols: syms,
        ids: ids_of,
        stress,
    })
}
