//! One word: CMUdict pronunciation -> phones of the inventory (a port of
//! dizione's `convert.py`). Many rules depend on how the vowel is written, so
//! they work on the pronunciation aligned to the spelling.

use super::align::base;
use super::symbols::{is_phone, is_vowel, Phone};

const ARPA_VOWELS: [&str; 15] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW",
];
const SIBILANTS: [&str; 6] = ["S", "Z", "SH", "ZH", "CH", "JH"];
const FORCE_SPELLINGS: [&str; 10] = [
    "forc", "ford", "sword", "worn", "torn", "sworn", "borne", "shorn", "pork", "divorc",
];
const PREFIXES: [&str; 8] = ["dis", "mis", "un", "in", "re", "non", "over", "pre"];
const WEAK_PREFIXES: [&str; 4] = ["be", "de", "re", "pre"];
const IRE_ENDINGS: [&str; 8] = ["ing", "ings", "ingly", "able", "y", "er", "ers", "est"];

fn consonant(name: &str) -> Option<&'static str> {
    Some(match name {
        "B" => "b",
        "CH" => "tʃ",
        "D" => "d",
        "DH" => "ð",
        "F" => "f",
        "G" => "ɡ",
        "HH" => "h",
        "JH" => "dʒ",
        "K" => "k",
        "L" => "l",
        "M" => "m",
        "N" => "n",
        "NG" => "ŋ",
        "P" => "p",
        "R" => "ɹ",
        "S" => "s",
        "SH" => "ʃ",
        "T" => "t",
        "TH" => "θ",
        "V" => "v",
        "W" => "w",
        "Y" => "j",
        "Z" => "z",
        "ZH" => "ʒ",
        _ => return None,
    })
}

fn full_vowel(name: &str) -> &'static str {
    match name {
        "AA" => "ɑː",
        "AE" => "æ",
        "AH" => "ʌ",
        "AO" => "ɔː",
        "AW" => "aʊ",
        "AY" => "aɪ",
        "EH" => "ɛ",
        "ER" => "ɜː",
        "EY" => "eɪ",
        "IH" => "ɪ",
        "IY" => "iː",
        "OW" => "oʊ",
        "OY" => "ɔɪ",
        "UH" => "ʊ",
        _ => "uː", // UW
    }
}

/// True when every symbol is a CMUdict symbol.
pub fn is_arpabet(symbols: &[&str]) -> bool {
    !symbols.is_empty()
        && symbols.iter().all(|s| {
            consonant(s).is_some()
                || (s.len() >= 2
                    && s.ends_with(['0', '1', '2'])
                    && ARPA_VOWELS.contains(&&s[..s.len() - 1]))
        })
}

fn vowel_letters(run: &str) -> String {
    run.chars().filter(|c| "aeiouy".contains(*c)).collect()
}

/// "h ə l ˈoʊ" -> phones; an error for a symbol outside the inventory.
pub fn from_text(text: &str) -> Result<Vec<Phone>, String> {
    let mut phones = Vec::new();
    for unit in text.split_whitespace() {
        let (stress, phone) = if let Some(p) = unit.strip_prefix('ˈ') {
            (1u8, p)
        } else if let Some(p) = unit.strip_prefix('ˌ') {
            (2u8, p)
        } else {
            (0u8, unit)
        };
        if !is_phone(phone) {
            return Err(format!("{phone:?} is not a phone of the inventory"));
        }
        if stress != 0 && !is_vowel(phone) {
            return Err(format!("stress mark on the consonant {phone:?}"));
        }
        phones.push((phone.to_string(), stress));
    }
    Ok(phones)
}

struct Word {
    word: Vec<char>,
    bases: Vec<String>,
    stress: Vec<Option<u8>>,
    aligned: bool,
    runs: Vec<String>,
    starts: Vec<usize>,
    count: usize,
}

impl Word {
    fn new(word: &str, arpabet: &[String], runs: Option<Vec<String>>) -> Result<Self, String> {
        let bases: Vec<String> = arpabet.iter().map(|s| base(s).to_string()).collect();
        let stress = arpabet
            .iter()
            .map(|s| {
                s.chars()
                    .last()
                    .and_then(|c| c.to_digit(10))
                    .map(|d| d as u8)
            })
            .collect();
        for (symbol, name) in arpabet.iter().zip(&bases) {
            if consonant(name).is_none() && !ARPA_VOWELS.contains(&name.as_str()) {
                return Err(format!("{symbol:?} is not an ARPABET symbol"));
            }
        }
        let aligned = runs.is_some();
        let runs = runs.unwrap_or_else(|| vec![String::new(); arpabet.len()]);
        let mut starts = Vec::new();
        let mut position = 0;
        for run in &runs {
            starts.push(position);
            position += run.chars().count();
        }
        let count = bases.len();
        Ok(Self {
            word: word.chars().collect(),
            bases,
            stress,
            aligned,
            runs,
            starts,
            count,
        })
    }
    fn s(&self, a: usize, b: usize) -> String {
        let (a, b) = (a.min(self.word.len()), b.min(self.word.len()));
        self.word[a..b.max(a)].iter().collect()
    }
    fn whole(&self) -> String {
        self.word.iter().collect()
    }
    fn base(&self, i: isize) -> Option<&str> {
        (0 <= i && (i as usize) < self.count).then(|| self.bases[i as usize].as_str())
    }
    fn vowel(&self, i: isize) -> bool {
        self.base(i).is_some_and(|b| ARPA_VOWELS.contains(&b))
    }
    fn st(&self, i: isize) -> u8 {
        self.stress[i as usize].unwrap_or(0)
    }
    fn unstressed(&self, i: isize) -> bool {
        self.vowel(i) && self.st(i) == 0
    }
    fn letters(&self, i: usize) -> String {
        vowel_letters(&self.runs[i])
    }
    fn before(&self, i: usize) -> String {
        if self.aligned {
            self.s(0, self.starts[i])
        } else {
            String::new()
        }
    }
    fn after(&self, i: usize) -> String {
        if self.aligned {
            self.s(
                self.starts[i] + self.runs[i].chars().count(),
                self.word.len(),
            )
        } else {
            String::new()
        }
    }
    fn ends_with(&self, i: usize, symbols: &[&str]) -> bool {
        let tail: Vec<&str> = self.bases.iter().skip(i + 1).map(String::as_str).collect();
        tail == symbols
    }
    fn first_vowel(&self, i: usize) -> bool {
        !self.bases[..i]
            .iter()
            .any(|b| ARPA_VOWELS.contains(&b.as_str()))
    }
    fn next_vowel_stress(&self, i: usize) -> Option<u8> {
        for j in i + 1..self.count {
            if ARPA_VOWELS.contains(&self.bases[j].as_str()) {
                return Some(self.stress[j].unwrap_or(0));
            }
        }
        None
    }
    fn starts_with(&self, prefixes: &[&str]) -> bool {
        let w = self.whole();
        prefixes.iter().any(|p| w.starts_with(p))
    }
    fn weak_prefix(&self, i: usize) -> bool {
        self.aligned
            && self.first_vowel(i)
            && self.letters(i) == "e"
            && self.starts_with(&WEAK_PREFIXES)
            && matches!(self.before(i).as_str(), "b" | "d" | "r" | "pr")
    }
}

fn north_or_force(w: &Word, i: usize, before_vowel: bool) -> &'static str {
    if !w.aligned {
        return "ɔː";
    }
    let spelled = w.letters(i);
    if matches!(spelled.as_str(), "oa" | "oo" | "ou") {
        return "oː";
    }
    if spelled != "o" {
        return "ɔː";
    }
    let run = &w.runs[i];
    let after_o: String = match run.chars().position(|c| c == 'o') {
        Some(p) => run.chars().skip(p + 1).collect(),
        None => run.clone(),
    };
    let tail = format!("{after_o}{}", w.after(i));
    let rest: String = tail.chars().skip(1).collect();
    if !before_vowel {
        if rest.starts_with('e') {
            return "oː";
        }
        if w.before(i).ends_with('p') && rest.starts_with('t') {
            return "oː";
        }
        let whole = w.whole();
        return if FORCE_SPELLINGS.iter().any(|s| whole.contains(s)) {
            "oː"
        } else {
            "ɔː"
        };
    }
    if rest.starts_with('r') {
        return "ɔː";
    }
    if matches!(
        rest.as_str(),
        "y" | "ies" | "ied" | "a" | "as" | "ah" | "er" | "ers"
    ) || ["ia", "io", "iu", "ing", "age"]
        .iter()
        .any(|p| rest.starts_with(p))
    {
        return "oː";
    }
    "ɔː"
}

fn weak_i(w: &Word, i: usize) -> &'static str {
    let (ii, nxt, nxt2) = (i as isize, w.base(i as isize + 1), w.base(i as isize + 2));
    if nxt == Some("F") && nxt2 == Some("AH") && w.base(ii + 3) == Some("L") {
        return "i";
    }
    if (nxt == Some("T") && nxt2 == Some("IY"))
        || (nxt == Some("B") && matches!(nxt2, Some("AH") | Some("L")))
        || (nxt == Some("F") && nxt2 == Some("AY"))
    {
        return "ᵻ";
    }
    if nxt == Some("T")
        && w.ends_with(i, &["T"])
        && matches!(w.base(ii - 1), Some("R") | Some("ER"))
        && w.whole().ends_with("ite")
    {
        return "ᵻ";
    }
    "ɪ"
}

fn weak_e(w: &Word, i: usize) -> &'static str {
    let ii = i as isize;
    let (nxt, prev) = (w.base(ii + 1), w.base(ii - 1));
    if nxt == Some("T") && (w.ends_with(i, &["T"]) || w.ends_with(i, &["T", "S"])) {
        return if prev == Some("L") {
            "ə"
        } else if prev == Some("R") {
            "ᵻ"
        } else {
            "ɪ"
        };
    }
    if nxt == Some("S") && (w.ends_with(i, &["S", "T"]) || w.ends_with(i, &["S", "T", "L", "IY"])) {
        return "ɪ";
    }
    if nxt == Some("JH") {
        return "ɪ";
    }
    if nxt == Some("K") && w.base(ii + 2) == Some("T") {
        return "ɛ";
    }
    if nxt == Some("D") && w.ends_with(i, &["D"]) {
        return "ɪ";
    }
    "ə"
}

fn inflection(w: &Word, i: usize) -> bool {
    if i + 1 >= w.count || (w.aligned && !format!("{}{}", w.runs[i], w.runs[i + 1]).contains('e')) {
        return false;
    }
    let ii = i as isize;
    let (nxt, prev) = (w.base(ii + 1), w.base(ii - 1));
    if nxt == Some("D")
        && matches!(prev, Some("T") | Some("D"))
        && (w.ends_with(i, &["D"]) || w.ends_with(i, &["D", "L", "IY"]))
    {
        return true;
    }
    nxt == Some("Z") && prev.is_some_and(|p| SIBILANTS.contains(&p)) && w.ends_with(i, &["Z"])
}

fn first_syllable(w: &Word, i: usize) -> Option<&'static str> {
    if !w.aligned || !w.first_vowel(i) || w.next_vowel_stress(i).unwrap_or(0) == 0 {
        return None;
    }
    let word = &w.word;
    let at = |k: usize| word.get(k).copied();
    let is_vl = |c: Option<char>| c.is_some_and(|c| "aeiouy".contains(c));
    if i == 0 && at(0) == Some('e') {
        if at(1) == Some('x') {
            return Some("ɛ");
        }
        if matches!(at(1), Some('n') | Some('m') | Some('s'))
            && at(2).is_some()
            && !is_vl(at(2))
            && at(2) != at(1)
        {
            return Some("ɛ");
        }
        return Some("ɪ");
    }
    if w.weak_prefix(i) {
        if w.s(0, 3) == "pre" || (w.s(0, 2) == "be" && matches!(at(2), Some('c') | Some('g'))) {
            return Some("ɪ");
        }
        return Some("ᵻ");
    }
    if w.letters(i) == "i"
        && w.s(0, 2) == "di"
        && word.len() > 3
        && !is_vl(at(2))
        && at(2) != Some('s')
        && is_vl(at(3))
    {
        return Some("ᵻ");
    }
    None
}

fn reduced(w: &Word, i: usize) -> &'static str {
    let ii = i as isize;
    let (name, spelled) = (w.bases[i].as_str(), w.letters(i));
    let (nxt, prev) = (w.base(ii + 1), w.base(ii - 1));
    if name == "IH" && nxt == Some("NG") {
        return "ɪ";
    }
    if inflection(w, i) {
        return "ᵻ";
    }
    if name == "AH" {
        let prev_run = if i > 0 {
            w.runs[i - 1].clone()
        } else {
            String::new()
        };
        if prev == Some("Y") && format!("{prev_run}{}", w.runs[i]).contains('u') {
            return "ʊ";
        }
        let whole = w.whole();
        if i == 0 && whole.starts_with('a') {
            return "ɐ";
        }
        if i == 0 && nxt == Some("N") && whole.starts_with("un") {
            return "ʌ";
        }
        if spelled == "a" && PREFIXES.contains(&w.before(i).as_str()) && w.runs[i].starts_with('a')
        {
            return "ɐ";
        }
    }
    if let Some(prefix) = first_syllable(w, i) {
        return prefix;
    }
    if spelled == "e"
        && matches!(nxt, Some("K") | Some("G"))
        && w.runs.get(i + 1).is_some_and(|r| r.starts_with('x'))
        && w.next_vowel_stress(i).unwrap_or(0) != 0
    {
        return "ɛ";
    }
    if matches!(spelled.as_str(), "i" | "ai" | "ui" | "y") {
        return weak_i(w, i);
    }
    if spelled == "e" {
        return weak_e(w, i);
    }
    if spelled == "a" {
        if nxt == Some("JH") || (nxt == Some("N") && w.base(ii + 2) == Some("JH")) {
            return "ɪ";
        }
        if nxt == Some("S") && w.ends_with(i, &["S"]) && w.whole().ends_with("ace") {
            return "ɪ";
        }
        if name == "IH" && !(nxt == Some("T") && w.ends_with(i, &["T"])) {
            return "ɪ";
        }
        return "ə";
    }
    if name == "AH" && nxt == Some("M") && w.ends_with(i, &["M"]) && w.whole().ends_with("ome") {
        return "ʌ";
    }
    if name == "IH" && !matches!(spelled.as_str(), "o" | "u" | "ou") {
        "ɪ"
    } else {
        "ə"
    }
}

fn unstressed_iy(w: &Word, i: usize) -> (&'static str, u8) {
    let ii = i as isize;
    let nxt = w.base(ii + 1);
    if nxt.is_none() || w.ends_with(i, &["Z"]) || w.ends_with(i, &["D"]) {
        return ("i", 0);
    }
    if w.vowel(ii + 1) {
        return (
            if nxt == Some("IH") && w.base(ii + 2) == Some("NG") {
                "i"
            } else {
                "ɪ"
            },
            0,
        );
    }
    if w.weak_prefix(i) {
        return if w.next_vowel_stress(i) == Some(1) {
            ("ᵻ", 0)
        } else {
            ("iː", 2)
        };
    }
    ("ɪ", 0)
}

fn emit(out: &mut Vec<(String, u8)>, phone: &str, stress: u8) {
    out.push((phone.to_string(), stress));
}

/// Citation form of `word`; `arpabet` is a CMUdict pronunciation and `runs`
/// the letters aligned to each symbol, or `None`.
pub fn convert(
    word: &str,
    arpabet: &[String],
    runs: Option<Vec<String>>,
) -> Result<Vec<Phone>, String> {
    let w = Word::new(word, arpabet, runs)?;
    let mut out: Vec<(String, u8)> = Vec::new();
    let mut i: usize = 0;
    while i < w.count {
        let ii = i as isize;
        let name = w.bases[i].as_str();
        let stress = w.stress[i].unwrap_or(0);
        let nxt = w.base(ii + 1);
        if !ARPA_VOWELS.contains(&name) {
            if name == "Y"
                && nxt == Some("AH")
                && w.unstressed(ii + 1)
                && matches!(w.base(ii - 1), Some("N") | Some("L"))
                && w.base(ii + 2) != Some("L")
                && (!w.aligned || format!("{}{}", w.runs[i], w.runs[i + 1]).contains('i'))
            {
                emit(&mut out, "iə", 0);
                i += 2;
                continue;
            }
            emit(&mut out, consonant(name).ok_or("not a consonant")?, 0);
            i += 1;
            continue;
        }
        let before_vowel = w.vowel(ii + 2);
        let prev_run_u =
            |w: &Word| i > 0 && format!("{}{}", w.runs[i - 1], w.runs[i]).contains('u');
        if name == "ER" {
            emit(&mut out, if stress != 0 { "ɜː" } else { "ɚ" }, stress);
            if nxt == Some("R") {
                i += 1;
            }
            if w.vowel(i as isize + 1) {
                emit(&mut out, "ɹ", 0);
            }
        } else if nxt == Some("R") && name == "AW" {
            emit(&mut out, "aʊ", stress);
            emit(&mut out, "ɚ", 0);
            if before_vowel {
                emit(&mut out, "ɹ", 0);
            }
            i += 1;
        } else if nxt == Some("R")
            && matches!(name, "AA" | "AO" | "IH" | "IY" | "EH" | "UH" | "UW" | "AY")
        {
            let (quality, single): (String, String) = match name {
                "AA" => ("ɑː".into(), "ɑːɹ".into()),
                "AO" => {
                    let q = north_or_force(&w, i, before_vowel);
                    (q.into(), format!("{q}ɹ"))
                }
                "IH" | "IY" => (
                    if stress != 0 && matches!(w.letters(i).as_str(), "e" | "eo") {
                        "iə".into()
                    } else {
                        "ɪ".into()
                    },
                    "ɪɹ".into(),
                ),
                "EH" => {
                    let doubled = format!("{}{}", w.runs[i + 1], w.after(i + 1)).starts_with("rr")
                        || (w.runs[i].ends_with('r') && w.runs[i + 1].starts_with('r'));
                    (
                        if w.letters(i) == "a" && doubled {
                            "æ".into()
                        } else {
                            "ɛ".into()
                        },
                        "ɛɹ".into(),
                    )
                }
                "UH" | "UW" => ("ʊɹ".into(), "ʊɹ".into()),
                _ => (
                    if !w.aligned || IRE_ENDINGS.contains(&w.after(i + 1).as_str()) {
                        "aɪɚ".into()
                    } else {
                        "aɪ".into()
                    },
                    "aɪɚ".into(),
                ),
            };
            if before_vowel {
                emit(&mut out, &quality, stress);
                emit(&mut out, "ɹ", 0);
            } else {
                emit(&mut out, &single, stress);
            }
            i += 1;
        } else if name == "AY"
            && nxt == Some("ER")
            && w.unstressed(ii + 1)
            && w.runs[i + 1] == "er"
            && w.runs[i] != "i"
        {
            emit(&mut out, "aɪ", stress);
        } else if name == "AY" && nxt == Some("ER") && w.unstressed(ii + 1) {
            emit(&mut out, "aɪɚ", stress);
            i += 1;
            if w.vowel(i as isize + 1) {
                emit(&mut out, "ɹ", 0);
            }
        } else if name == "AY" && nxt == Some("AH") && w.unstressed(ii + 1) {
            emit(&mut out, "aɪə", stress);
            i += 1;
        } else if name == "IY" && nxt == Some("AH") && w.unstressed(ii + 1) && !w.weak_prefix(i) {
            let after = w.base(ii + 2);
            if stress != 1
                && (matches!(after, Some("L") | Some("D") | Some("T"))
                    || (after == Some("N") && w.base(ii + 3) == Some("S")))
            {
                emit(&mut out, "ɪ", 0);
            } else {
                emit(&mut out, "iə", stress);
                i += 1;
            }
        } else if stress == 0
            && name == "AH"
            && nxt == Some("N")
            && w.base(ii - 1) == Some("T")
            && w.base(ii + 2) != Some("T")
            && out.len() >= 2
            && out[out.len() - 1].0 == "t"
            && is_vowel(&out[out.len() - 2].0)
            && out[out.len() - 2].1 != 0
        {
            let last = out.len() - 1;
            out[last].0 = "ʔ".into();
            emit(&mut out, "n̩", 0);
            i += 1;
        } else if stress == 0
            && name == "AH"
            && nxt == Some("W")
            && w.base(ii + 2) == Some("AH")
            && w.unstressed(ii + 2)
            && format!(
                "{}{}",
                w.runs[i],
                w.runs.get(i + 1).cloned().unwrap_or_default()
            )
            .contains('u')
        {
            emit(&mut out, "uː", 0);
            i += 1;
        } else if stress == 0
            && name == "AH"
            && nxt == Some("L")
            && i > 0
            && !(w.vowel(ii + 2) && w.st(ii + 2) != 0)
        {
            if w.letters(i) == "i" && w.vowel(ii + 2) {
                emit(&mut out, "ɪ", 0);
            } else if w.base(ii - 1) == Some("Y") && prev_run_u(&w) {
                emit(&mut out, "ʊ", 0);
            } else {
                emit(&mut out, "əl", 0);
                i += 1;
            }
        } else if stress == 0 && matches!(name, "AH" | "IH") {
            emit(&mut out, reduced(&w, i), 0);
        } else if name == "IY" && (stress == 0 || (stress == 2 && nxt.is_none())) {
            let (p, s) = unstressed_iy(&w, i);
            emit(&mut out, p, s);
        } else if stress == 0
            && name == "AE"
            && i == 0
            && w.starts_with(&["ad", "acc"])
            && matches!(nxt, Some("D") | Some("K"))
        {
            emit(&mut out, "ɐ", 0);
        } else if stress == 0 && name == "ER" {
            emit(&mut out, "ɚ", 0);
        } else if matches!(name, "AA" | "AO")
            && w.letters(i) == "a"
            && (w.before(i).ends_with('w') || w.before(i).ends_with("qu"))
            && nxt == Some("N")
            || name == "AO" && w.letters(i) == "o"
        {
            emit(&mut out, "ɔ", stress);
        } else {
            emit(&mut out, full_vowel(name), stress);
        }
        i += 1;
    }
    flap(&mut out);
    for k in 0..out.len().saturating_sub(1) {
        if out[k].0 == "n" && matches!(out[k + 1].0.as_str(), "k" | "ɡ") {
            out[k].0 = "ŋ".into();
        }
    }
    settle_in_place(&mut out);
    Ok(out)
}

/// t between a vowel (or r) and an unstressed vowel: water, city, party.
fn flap(out: &mut [(String, u8)]) {
    for k in 1..out.len().saturating_sub(1) {
        if out[k].0 != "t" {
            continue;
        }
        let before = out[k - 1].0.clone();
        let (after, stress) = out[k + 1].clone();
        if (!is_vowel(&before) && before != "ɹ")
            || before == "n̩"
            || before == "əl"
            || !is_vowel(&after)
            || after == "n̩"
        {
            continue;
        }
        let final_o =
            after == "oʊ" && (k + 2 == out.len() || (k + 3 == out.len() && out[k + 2].0 == "z"));
        if stress != 0 && !final_o {
            continue;
        }
        if (after == "ə" || after == "ɚ") && k + 2 < out.len() && out[k + 2].0 == "n" {
            continue;
        }
        out[k].0 = "ɾ".into();
    }
}

/// `settle_in_place` for a finished pronunciation (compounds, prefixed words).
pub fn settle_stress(phones: &[Phone]) -> Vec<Phone> {
    let mut out = phones.to_vec();
    settle_in_place(&mut out);
    out
}

/// One primary stress per word, and no secondary stress next to the primary one.
fn settle_in_place(out: &mut [(String, u8)]) {
    let vowels: Vec<usize> = (0..out.len()).filter(|&k| is_vowel(&out[k].0)).collect();
    let primaries: Vec<usize> = vowels.iter().copied().filter(|&k| out[k].1 == 1).collect();
    if primaries.len() > 1 {
        for &k in &primaries[..primaries.len() - 1] {
            out[k].1 = 2;
        }
    }
    for (position, &k) in vowels.iter().enumerate() {
        if out[k].1 != 2 {
            continue;
        }
        let mut neighbours = Vec::new();
        if position > 0 {
            neighbours.push(vowels[position - 1]);
        }
        if position + 1 < vowels.len() {
            neighbours.push(vowels[position + 1]);
        }
        if neighbours.iter().any(|&n| out[n].1 == 1) {
            out[k].1 = 0;
        }
    }
}
