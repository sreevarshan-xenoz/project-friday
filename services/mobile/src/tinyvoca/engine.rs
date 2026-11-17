//! The phonemizer: text -> spoken words of phones, clause by clause (a port of
//! dizione's `engine.py`).
//!
//! Order of authority for one word: inline pronunciation, the exception list
//! (a name of several words is spoken with the stress written), function words
//! and function-word groups, the dictionary built from CMUdict, regular endings
//! and compounds of dictionary words, names in "-leigh" and words in "-virus",
//! and last the unknown-word model.

use super::align::Aligner;
use super::convert::{convert, from_text, settle_stress};
use super::g2p::Model;
use super::homographs::Homographs;
use super::morphology;
use super::normalize::{Normalizer, Token, Word};
use super::sentence::{self, Unit};
use super::symbols::{tokenize, Phone, Phonemized, Piece};
use super::tables::{self, Pron};
use fancy_regex::Regex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub enum Analysed {
    Punct(String),
    Clause(Vec<Unit>),
}

pub struct Phonemizer {
    lexicon: HashMap<String, String>,
    aligner: Aligner,
    model: Option<Model>,
    homographs: Option<Homographs>,
    functions: HashMap<String, tables::FunctionWord>,
    phrases: HashMap<Vec<String>, tables::Phrase>,
    phrase_length: usize,
    exceptions: HashMap<String, Pron>,
    names: HashMap<Vec<String>, Vec<Pron>>,
    name_starts: HashSet<String>,
    name_length: usize,
    letter_names: HashMap<char, Vec<Phone>>,
    normalizer: Normalizer,
    cache: RefCell<HashMap<String, (Vec<Phone>, &'static str)>>,
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    std::fs::read(dir.join(name)).map_err(|e| format!("{}: {e}", dir.join(name).display()))
}

fn text(dir: &Path, name: &str) -> Result<String, String> {
    String::from_utf8(read(dir, name)?).map_err(|e| format!("{name}: {e}"))
}

impl Phonemizer {
    /// Loads the built files from `dir` (the `phonemizer/` folder of the model).
    pub fn new(dir: &Path) -> Result<Self, String> {
        let lexicon = text(dir, "lexicon.tsv")?
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let align: HashMap<String, HashMap<String, f64>> =
            serde_json::from_str(&text(dir, "align.json")?)
                .map_err(|e| format!("align.json: {e}"))?;
        let model = Model::new(&text(dir, "g2p.json")?, &read(dir, "g2p.npz")?)?;
        let homographs = Homographs::new(&text(dir, "homographs.json")?)?;
        let functions = tables::function_words()?;
        let phrases = tables::phrases()?;
        let exceptions = tables::pronunciations(tables::EXCEPTIONS)?;
        let names = tables::names(tables::EXCEPTIONS)?;
        let mut letter_names = HashMap::new();
        for (k, v) in tables::mapping(tables::LETTERS) {
            if let Some(c) = k.chars().next() {
                letter_names.insert(c, from_text(&v)?);
            }
        }
        Ok(Self {
            lexicon,
            aligner: Aligner::new(align),
            model: Some(model),
            homographs: Some(homographs),
            phrase_length: phrases.keys().map(Vec::len).max().unwrap_or(1),
            functions,
            phrases,
            exceptions,
            name_starts: names.keys().map(|k| k[0].clone()).collect(),
            name_length: names.keys().map(Vec::len).max().unwrap_or(1),
            names,
            letter_names,
            normalizer: Normalizer::new()?,
            cache: RefCell::new(HashMap::new()),
        })
    }

    // ---- single words -------------------------------------------------

    fn entry(&self, word: &str, entry: &Pron) -> Option<Vec<Phone>> {
        match entry {
            Pron::Phones(p) => Some(p.clone()),
            Pron::Arpabet(symbols) => {
                convert(word, symbols, self.aligner.align(word, symbols)).ok()
            }
        }
    }

    /// Pronunciation from the exceptions or the dictionary.
    fn known(&self, word: &str) -> Option<Vec<Phone>> {
        if let Some(e) = self.exceptions.get(word) {
            return self.entry(word, e);
        }
        self.lexicon.get(word).and_then(|t| from_text(t).ok())
    }

    /// A dictionary word that may be half of a compound; CMUdict is full of
    /// surnames, so a word also listed with a regular ending counts as ordinary.
    fn ordinary(&self, word: &str) -> Option<Vec<Phone>> {
        if word.chars().count() < 3
            || !["s", "es", "ed", "ing"]
                .iter()
                .any(|e| self.lexicon.contains_key(&format!("{word}{e}")))
        {
            return None;
        }
        self.known(word)
    }

    /// The unknown-word model, then the conversion rules, on one spelling.
    fn guess(&self, word: &str) -> Vec<Phone> {
        let Some(model) = &self.model else {
            return self.spell(word);
        };
        let spelling = model.spellable(word);
        let mut symbols = model.predict(&spelling);
        // An r with no vowel beside it is a syllable: write it as one.
        for k in 0..symbols.len() {
            if symbols[k] == "R" {
                let vowel = |j: isize| {
                    j >= 0
                        && (j as usize) < symbols.len()
                        && symbols[j as usize].ends_with(['0', '1', '2'])
                };
                if !vowel(k as isize - 1) && !vowel(k as isize + 1) {
                    symbols[k] = "ER0".into();
                }
            }
        }
        if !symbols.iter().any(|s| s.ends_with(['0', '1', '2'])) {
            return self.spell(word);
        }
        // Every word spoken alone has a main stress, even when the model marked none.
        match convert(&spelling, &symbols, self.aligner.align(&spelling, &symbols)) {
            Ok(phones) => sentence::promote(&phones),
            Err(_) => self.spell(word),
        }
    }

    /// Letter names, joined: the last letter carries the main stress.
    fn spell(&self, letters: &str) -> Vec<Phone> {
        let lower = letters.to_lowercase();
        let possessive = lower.ends_with("'s");
        let body = if possessive {
            &lower[..lower.len() - 2]
        } else {
            &lower[..]
        };
        let names: Vec<&Vec<Phone>> = body
            .chars()
            .filter_map(|c| self.letter_names.get(&c))
            .collect();
        let mut out = Vec::new();
        for (index, name) in names.iter().enumerate() {
            out.extend(name.iter().map(|(phone, stress)| {
                (
                    phone.clone(),
                    if *stress > 0 && index + 1 < names.len() {
                        2
                    } else {
                        *stress
                    },
                )
            }));
        }
        if possessive && !out.is_empty() {
            morphology::plural(&out)
        } else {
            out
        }
    }

    fn is_word(&self, word: &str) -> bool {
        self.lexicon.contains_key(word)
            || self.functions.contains_key(word)
            || self.exceptions.contains_key(word)
    }

    /// Two kinds of new word the model reads badly: a name in "-leigh" is said
    /// like the same name in "-ley"; a word in "-virus" is its first part and "virus".
    fn by_parts(&self, word: &str) -> Option<(Vec<Phone>, &'static str)> {
        if word.ends_with("'s") && word.len() > 3 {
            let (phones, source) = self.by_parts(&word[..word.len() - 2])?;
            return Some((morphology::plural(&phones), source));
        }
        static LEIGH: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let leigh = LEIGH.get_or_init(|| Regex::new(r"(?<=[a-z]{2})leigh(?=s?$)").expect("regex"));
        if leigh.is_match(word).unwrap_or(false) {
            return Some(self.word(&leigh.replace_all(word, "ley")));
        }
        for tail in ["viruses", "virus"] {
            if word.ends_with(tail) && word.len() - tail.len() > 2 {
                if let Some(second) = self.known(tail) {
                    let (first, source) = self.word(&word[..word.len() - tail.len()]);
                    let phones =
                        sentence::promote(&settle_stress(&morphology::compound(&first, &second)));
                    return Some((phones, if source == "model" { "model" } else { "ending" }));
                }
            }
        }
        None
    }

    /// Phones and where they came from for one spelling in lower case.
    pub fn word(&self, word: &str) -> (Vec<Phone>, &'static str) {
        if let Some(hit) = self.cache.borrow().get(word) {
            return hit.clone();
        }
        let mut found = self.known(word).map(|p| (p, "dictionary"));
        if found.is_none() {
            found = morphology::derive(word, &|w| self.known(w), &|w| self.ordinary(w))
                .map(|p| (settle_stress(&p), "ending"));
        }
        if found.is_none() {
            found = self.by_parts(word);
        }
        if found.is_none() && word.ends_with("'s") && word.len() > 3 {
            found = Some((
                morphology::plural(&self.guess(&word[..word.len() - 2])),
                "model",
            ));
        }
        let found = found.unwrap_or_else(|| (self.guess(word), "model"));
        self.cache
            .borrow_mut()
            .insert(word.to_string(), found.clone());
        found
    }

    // ---- sentences ----------------------------------------------------

    /// Units of one clause from its tokens. `written` is the whole text as
    /// tokens and `places` where each word sits in it, for the homograph rules.
    fn units(&self, words: &[Word], places: &[usize], written: &[String]) -> Vec<Unit> {
        let mut units = Vec::new();
        let mut index = 0;
        while index < words.len() {
            let token = &words[index];
            if let Some(phones) = &token.phones {
                units.push(Unit::new(
                    &token.text,
                    phones.clone(),
                    "p",
                    None,
                    token.attach,
                ));
            } else if token.spell {
                units.push(Unit::new(
                    &token.text,
                    self.spell(&token.text),
                    "p",
                    None,
                    token.attach,
                ));
            } else {
                let size = if self.name_starts.contains(&token.text) {
                    self.name(words, index)
                } else {
                    0
                };
                if size > 0 {
                    // a taught name: every word as written in the table
                    let key: Vec<String> = words[index..index + size]
                        .iter()
                        .map(|w| w.text.clone())
                        .collect();
                    for (position, (text, entry)) in key.iter().zip(&self.names[&key]).enumerate() {
                        units.push(Unit::new(
                            text,
                            self.entry(text, entry).unwrap_or_default(),
                            "p",
                            None,
                            token.attach && position == 0,
                        ));
                    }
                    index += size;
                    continue;
                }
                let size = self.phrase(words, index);
                if size > 0 {
                    let key: Vec<String> = words[index..index + size]
                        .iter()
                        .map(|w| w.text.clone())
                        .collect();
                    let (kind, forms, final_form) = &self.phrases[&key];
                    if forms.len() == 1 {
                        units.push(Unit::new(
                            &key.join(" "),
                            forms[0].clone(),
                            kind,
                            final_form.clone(),
                            token.attach,
                        ));
                    } else {
                        // the boundaries stay, only the stress is fixed
                        for (position, (text, form)) in key.iter().zip(forms).enumerate() {
                            units.push(Unit::new(
                                text,
                                form.clone(),
                                kind,
                                None,
                                token.attach && position == 0,
                            ));
                        }
                    }
                    index += size;
                    continue;
                }
                let homograph = self.homographs.as_ref().filter(|h| {
                    !written.is_empty()
                        && h.covers(&token.text)
                        && !self.exceptions.contains_key(&token.text)
                });
                if let Some(h) = homograph {
                    let (phones, ending) =
                        h.pronounce(&token.text, written, places[index], token.attach);
                    let phones = from_text(&phones).unwrap_or_default();
                    units.push(Unit::new(
                        &token.text,
                        if ending {
                            morphology::plural(&phones)
                        } else {
                            phones
                        },
                        "c",
                        None,
                        token.attach,
                    ));
                } else if let Some((kind, form, final_form)) = self.functions.get(&token.text) {
                    units.push(Unit::new(
                        &token.text,
                        form.clone(),
                        kind,
                        final_form.clone(),
                        token.attach,
                    ));
                } else {
                    let (phones, _) = self.word(&token.text);
                    units.push(Unit::new(&token.text, phones, "c", None, token.attach));
                }
            }
            index += 1;
        }
        units
    }

    fn name(&self, words: &[Word], index: usize) -> usize {
        for size in (2..=self.name_length.min(words.len() - index)).rev() {
            let span = &words[index..index + size];
            let key: Vec<String> = span.iter().map(|w| w.text.clone()).collect();
            if self.names.contains_key(&key)
                && span.iter().all(|w| w.phones.is_none() && !w.spell)
                && !span[1..].iter().any(|w| w.attach)
            {
                return size;
            }
        }
        0
    }

    fn phrase(&self, words: &[Word], index: usize) -> usize {
        for size in (2..=self.phrase_length.min(words.len() - index)).rev() {
            let span = &words[index..index + size];
            if span[1..]
                .iter()
                .all(|w| w.phones.is_none() && !w.spell && !w.attach)
                && span[0].phones.is_none()
                && !span[0].spell
                && self
                    .phrases
                    .contains_key(&span.iter().map(|w| w.text.clone()).collect::<Vec<_>>())
            {
                return size;
            }
        }
        0
    }

    /// Clauses and marks in order.
    pub fn analyse(&self, text: &str) -> Vec<Analysed> {
        let stream = self.normalizer.tokens(text, &|w| self.is_word(w));
        // The text as the homograph rules see it; a word given only as phones stands as "x".
        let written: Vec<String> = stream
            .iter()
            .map(|t| match t {
                Token::Punct(m) => m.clone(),
                Token::Word(w) => {
                    if !w.raw.is_empty() {
                        w.raw.clone()
                    } else if !w.text.is_empty() {
                        w.text.clone()
                    } else {
                        "x".into()
                    }
                }
            })
            .collect();
        let mut out = Vec::new();
        let (mut words, mut places): (Vec<Word>, Vec<usize>) = (Vec::new(), Vec::new());
        let flush = |words: &mut Vec<Word>, places: &mut Vec<usize>, out: &mut Vec<Analysed>| {
            if !words.is_empty() {
                let mut units = self.units(words, places, &written);
                sentence::stress_clause(&mut units);
                sentence::join_clause(&mut units);
                out.push(Analysed::Clause(units));
                words.clear();
                places.clear();
            }
        };
        for (place, token) in stream.into_iter().enumerate() {
            match token {
                Token::Punct(mark) => {
                    flush(&mut words, &mut places, &mut out);
                    out.push(Analysed::Punct(mark));
                }
                Token::Word(w) => {
                    words.push(w);
                    places.push(place);
                }
            }
        }
        flush(&mut words, &mut places, &mut out);
        out
    }

    pub fn pieces(&self, text: &str) -> Vec<Piece> {
        self.analyse(text)
            .into_iter()
            .map(|a| match a {
                Analysed::Punct(m) => Piece::Punct(m),
                Analysed::Clause(units) => Piece::Words(sentence::groups(&units)),
            })
            .collect()
    }

    /// Text -> the tokens the model takes; `None` when there is nothing to say.
    pub fn phonemize(&self, text: &str) -> Option<Phonemized> {
        if text.trim().is_empty() {
            return None;
        }
        tokenize(&self.pieces(text))
    }
}
