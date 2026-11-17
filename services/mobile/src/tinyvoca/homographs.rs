//! Words spelled alike and pronounced differently: which reading is meant
//! (a port of dizione's `homographs.py`). A small linear classifier looks at
//! the neighbouring words; hand-written rules go first.

use super::tables::{rows, HOMOGRAPH_RULES};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const CLASSES: [(&str, &str); 13] = [
    ("TO", "to"),
    ("DET", "the a an this that these those every each no some any another such what which whose"),
    ("POSS", "my your his her its our their"),
    ("SUBJ", "i you he she it we they who"),
    ("OBJ", "me him us them"),
    ("MODAL", "will would shall should can could may might must 'll 'd won't can't cannot couldn't shouldn't wouldn't let's please"),
    ("DO", "do does did don't doesn't didn't"),
    ("BE", "am is are was were be been being isn't aren't wasn't weren't i'm you're we're they're he's she's it's that's there's"),
    ("HAVE", "have has had having haven't hasn't hadn't i've you've we've they've"),
    ("NEG", "not never"),
    (
        "PREP",
        "of in on at by for from with into onto over under about after before between through during without within against among around across behind beside near upon toward towards as than like",
    ),
    ("CONJ", "and or but nor so yet if because although though while when where unless until since whether"),
    ("ADV", "very too quite rather more most less least also just only even still always often sometimes usually again now then here there soon already almost"),
];
const WINDOW: usize = 6;

fn class_of(word: &str) -> Option<&'static str> {
    static TAGS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TAGS.get_or_init(|| {
        CLASSES
            .iter()
            .flat_map(|(n, w)| w.split_whitespace().map(move |x| (x, *n)))
            .collect()
    })
    .get(word)
    .copied()
}

fn first(token: &str) -> char {
    token.chars().next().unwrap_or(' ')
}

pub fn tag(token: &str) -> String {
    if token == "<s>" || token == "</s>" {
        return token.to_string();
    }
    let c = first(token);
    if !c.is_alphabetic() {
        return if c.is_numeric() { "NUM" } else { "PUNCT" }.to_string();
    }
    let lower = token.to_lowercase();
    if let Some(name) = class_of(&lower) {
        return name.to_string();
    }
    let n = lower.chars().count();
    for (ending, name) in [("'s", "POS"), ("ly", "LY"), ("ing", "ING"), ("ed", "ED")] {
        if lower.ends_with(ending) && n > ending.chars().count() + 1 {
            return name.to_string();
        }
    }
    if lower.ends_with('s') && !lower.ends_with("ss") && n > 3 {
        return "S".to_string();
    }
    if c.is_uppercase() { "CAP" } else { "W" }.to_string()
}

fn features(tokens: &[String], index: usize) -> Vec<String> {
    let at = |offset: isize| -> String {
        let p = index as isize + offset;
        if p >= 0 && (p as usize) < tokens.len() {
            tokens[p as usize].clone()
        } else if offset < 0 {
            "<s>".into()
        } else {
            "</s>".into()
        }
    };
    let (p2, p1, n1, n2) = (at(-2), at(-1), at(1), at(2));
    let (tp2, tp1, tn1, tn2) = (tag(&p2), tag(&p1), tag(&n1), tag(&n2));
    let mut out = vec![
        format!("tp1={tp1}"),
        format!("tp2={tp2}"),
        format!("tn1={tn1}"),
        format!("tn2={tn2}"),
        format!("tp2p1={tp2}+{tp1}"),
        format!("tp1n1={tp1}+{tn1}"),
        format!("tn1n2={tn1}+{tn2}"),
    ];
    if index == 0 {
        out.push("first".into());
    } else if first(&tokens[index]).is_uppercase() {
        out.push("cap".into());
    }
    out.push(format!("p1={}", p1.to_lowercase()));
    out.push(format!("p2={}", p2.to_lowercase()));
    out.push(format!("n1={}", n1.to_lowercase()));
    out.push(format!("n2={}", n2.to_lowercase()));
    let target = tokens[index].to_lowercase();
    let from = index.saturating_sub(WINDOW);
    let to = (index + WINDOW + 1).min(tokens.len());
    let mut nearby: Vec<String> = tokens[from..to]
        .iter()
        .filter(|t| first(t).is_alphabetic())
        .map(|t| t.to_lowercase())
        .filter(|t| *t != target)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    nearby.sort();
    out.extend(nearby.into_iter().map(|w| format!("w={w}")));
    out
}

fn shared(names: &[String]) -> impl Iterator<Item = &String> {
    names
        .iter()
        .filter(|n| n.starts_with('t') || *n == "first" || *n == "cap")
}

fn is_end(token: &str) -> bool {
    matches!(token, "." | "?" | "!")
}

/// The sentence around `tokens[index]` and the index inside it.
pub fn sentence_of(tokens: &[String], index: usize) -> (&[String], usize) {
    let mut start = index;
    while start > 0 && !is_end(&tokens[start - 1]) {
        start -= 1;
    }
    let mut end = index;
    while end < tokens.len() && !is_end(&tokens[end]) {
        end += 1;
    }
    (&tokens[start..(end + 1).min(tokens.len())], index - start)
}

const SKIP_FOR_BEFORE: [&str; 3] = ["ADV", "NEG", "LY"];
const SKIP_FOR_AUX: [&str; 7] = ["ADV", "NEG", "LY", "SUBJ", "OBJ", "DET", "POSS"];

fn left(tokens: &[String], index: usize, skip: &[&str]) -> String {
    let lowest = (index as isize - 5).max(0) as usize;
    for position in (lowest..index).rev() {
        let token = &tokens[position];
        if skip.contains(&tag(token).as_str()) {
            continue;
        }
        let lower = token.to_lowercase();
        return if lower.ends_with("'ll") {
            "will".into()
        } else if lower.ends_with("'ve") {
            "have".into()
        } else {
            lower
        };
    }
    "<s>".into()
}

#[derive(Debug, Clone)]
pub struct Sense {
    pub id: String,
    pub role: String,
    pub phones: String,
}

struct Entry {
    senses: Vec<Sense>,
    kind: Kind,
}

enum Kind {
    NounVerb {
        bias: f64,
        weights: HashMap<String, f64>,
    },
    Own {
        bias: Vec<f64>,
        weights: Vec<HashMap<String, f64>>,
    },
}

type Rules = HashMap<String, Vec<(String, Vec<(String, HashSet<String>)>)>>;

fn read_rules() -> Result<Rules, String> {
    let mut rules: Rules = HashMap::new();
    for (number, c) in rows(HOMOGRAPH_RULES) {
        if c.len() != 3 {
            return Err(format!(
                "homograph_rules.txt line {number}: expected word, sense, conditions"
            ));
        }
        let mut conditions = Vec::new();
        for part in c[2].split(';') {
            let part = part.trim();
            let (name, words) = part.split_once(':').unwrap_or((part, ""));
            let bare = words.trim().is_empty();
            if !["aux", "before", "next", "near", "first", "joined"].contains(&name)
                || (name == "first") != bare
            {
                return Err(format!(
                    "homograph_rules.txt line {number}: bad condition {part:?}"
                ));
            }
            conditions.push((
                name.to_string(),
                words.split_whitespace().map(String::from).collect(),
            ));
        }
        rules
            .entry(c[0].clone())
            .or_default()
            .push((c[1].clone(), conditions));
    }
    Ok(rules)
}

fn holds(
    condition: &str,
    words: &HashSet<String>,
    tokens: &[String],
    index: usize,
    joined: bool,
) -> bool {
    match condition {
        "first" => index == 0,
        "joined" => joined && index > 0 && words.contains(&tokens[index - 1].to_lowercase()),
        "aux" => words.contains(&left(tokens, index, &SKIP_FOR_AUX)),
        "before" => words.contains(&left(tokens, index, &SKIP_FOR_BEFORE)),
        "next" => words.contains(
            &tokens
                .get(index + 1)
                .map(|t| t.to_lowercase())
                .unwrap_or_else(|| "</s>".into()),
        ),
        _ => {
            let from = index.saturating_sub(WINDOW);
            let to = (index + WINDOW + 1).min(tokens.len());
            tokens[from..to]
                .iter()
                .any(|t| words.contains(&t.to_lowercase()))
        }
    }
}

fn weights_of(v: &Value) -> HashMap<String, f64> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, x)| x.as_f64().map(|f| (k.clone(), f)))
                .collect()
        })
        .unwrap_or_default()
}

pub struct Homographs {
    words: HashMap<String, Entry>,
    pooled_bias: f64,
    pooled: HashMap<String, f64>,
    plural_bases: HashSet<String>,
    rules: Rules,
}

impl Homographs {
    pub fn new(json: &str) -> Result<Self, String> {
        let data: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut words = HashMap::new();
        for (word, e) in data["words"].as_object().ok_or("homographs.json: words")? {
            let senses = e["senses"]
                .as_array()
                .ok_or("homographs.json: senses")?
                .iter()
                .map(|s| Sense {
                    id: s["id"].as_str().unwrap_or("").into(),
                    role: s["role"].as_str().unwrap_or("").into(),
                    phones: s["phones"].as_str().unwrap_or("").into(),
                })
                .collect();
            let m = &e["model"];
            let kind = if m["kind"] == "noun-verb" {
                Kind::NounVerb {
                    bias: m["bias"].as_f64().unwrap_or(0.0),
                    weights: weights_of(&m["weights"]),
                }
            } else {
                Kind::Own {
                    bias: m["bias"]
                        .as_array()
                        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                        .unwrap_or_default(),
                    weights: m["weights"]
                        .as_array()
                        .map(|a| a.iter().map(weights_of).collect())
                        .unwrap_or_default(),
                }
            };
            words.insert(word.clone(), Entry { senses, kind });
        }
        let rules = read_rules()?;
        for (word, word_rules) in &rules {
            for (sense, _) in word_rules {
                if !words
                    .get(word)
                    .is_some_and(|e| e.senses.iter().any(|s| &s.id == sense))
                {
                    return Err(format!(
                        "homograph rule for {word:?} names the unknown sense {sense:?}"
                    ));
                }
            }
        }
        Ok(Self {
            words,
            pooled_bias: data["pooled"]["bias"].as_f64().unwrap_or(0.0),
            pooled: weights_of(&data["pooled"]["weights"]),
            plural_bases: data["plural_bases"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            rules,
        })
    }

    pub fn contains(&self, word: &str) -> bool {
        self.words.contains_key(word)
    }

    /// True for a homograph and for the regular -s form of a noun-or-verb one.
    pub fn covers(&self, word: &str) -> bool {
        self.contains(word)
            || word
                .strip_suffix('s')
                .is_some_and(|b| self.plural_bases.contains(b))
    }

    /// (phones as text, needs the -s ending) for `tokens[index]`.
    pub fn pronounce(
        &self,
        word: &str,
        tokens: &[String],
        index: usize,
        joined: bool,
    ) -> (String, bool) {
        if self.contains(word) {
            return (
                self.choose(word, tokens, index, joined).phones.clone(),
                false,
            );
        }
        let base = &word[..word.len() - 1];
        let entry = &self.words[base];
        if let Some(n) = entry.senses.iter().find(|s| s.role == "n") {
            if n.id == "adj" {
                let v = entry
                    .senses
                    .iter()
                    .find(|s| s.role == "v")
                    .expect("a verb sense");
                return (v.phones.clone(), true);
            }
        }
        (
            self.choose(base, tokens, index, joined).phones.clone(),
            true,
        )
    }

    fn scores(&self, word: &str, tokens: &[String], index: usize) -> Vec<f64> {
        let entry = &self.words[word];
        let (sentence, position) = sentence_of(tokens, index);
        let names = features(sentence, position);
        let sum = |w: &HashMap<String, f64>, it: &mut dyn Iterator<Item = &String>| {
            it.map(|n| w.get(n).copied().unwrap_or(0.0)).sum::<f64>()
        };
        match &entry.kind {
            Kind::NounVerb { bias, weights } => {
                let mut v = self.pooled_bias + sum(&self.pooled, &mut shared(&names));
                v += bias + sum(weights, &mut names.iter());
                entry
                    .senses
                    .iter()
                    .map(|s| if s.role == "v" { v } else { -v })
                    .collect()
            }
            Kind::Own { bias, weights } => bias
                .iter()
                .zip(weights)
                .map(|(b, w)| b + sum(w, &mut names.iter()))
                .collect(),
        }
    }

    /// The sense meant by `tokens[index]`: hand rules first, then the classifier.
    pub fn choose(&self, word: &str, tokens: &[String], index: usize, joined: bool) -> &Sense {
        let entry = &self.words[word];
        let (sentence, position) = sentence_of(tokens, index);
        if let Some(word_rules) = self.rules.get(word) {
            for (sense, conditions) in word_rules {
                if conditions
                    .iter()
                    .all(|(name, words)| holds(name, words, sentence, position, joined))
                {
                    return entry
                        .senses
                        .iter()
                        .find(|s| &s.id == sense)
                        .expect("checked in new");
                }
            }
        }
        let scores = self.scores(word, tokens, index);
        let mut best = 0;
        for (i, s) in scores.iter().enumerate() {
            if *s > scores[best] {
                best = i;
            }
        }
        &entry.senses[best]
    }
}
