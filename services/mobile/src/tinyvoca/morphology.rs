//! Words the dictionary lacks but can be built from words it has (a port of
//! dizione's `morphology.py`): plurals, -ed, -ing, -ly, regular endings,
//! prefixes, compounds and British spellings.

use super::symbols::{is_vowel, Phone};

const SIBILANT: [&str; 6] = ["s", "z", "ʃ", "ʒ", "tʃ", "dʒ"];
const VOICELESS: [&str; 6] = ["p", "t", "k", "f", "θ", "ʔ"];
const CONSONANT_LETTERS: &str = "bcdfghjklmnpqrstvwxz";

pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<Vec<Phone>>;

fn p(phones: &[&str]) -> Vec<Phone> {
    phones.iter().map(|x| (x.to_string(), 0)).collect()
}
fn ph(phones: &[(&str, u8)]) -> Vec<Phone> {
    phones.iter().map(|(x, s)| (x.to_string(), *s)).collect()
}

fn split_r(phone: &str) -> Option<&'static str> {
    Some(match phone {
        "ɑːɹ" => "ɑː",
        "ɔːɹ" => "ɔː",
        "oːɹ" => "oː",
        "ɪɹ" => "ɪ",
        "ɛɹ" => "ɛ",
        _ => return None,
    })
}

fn endings() -> Vec<(&'static str, Vec<Phone>)> {
    vec![
        ("fully", p(&["f", "əl", "i"])),
        ("lessly", p(&["l", "ə", "s", "l", "i"])),
        ("ness", p(&["n", "ə", "s"])),
        ("less", p(&["l", "ə", "s"])),
        ("ments", p(&["m", "ə", "n", "t", "s"])),
        ("ment", p(&["m", "ə", "n", "t"])),
        ("ful", p(&["f", "əl"])),
        ("ship", ph(&[("ʃ", 0), ("ɪ", 2), ("p", 0)])),
        ("hood", ph(&[("h", 0), ("ʊ", 2), ("d", 0)])),
        ("like", ph(&[("l", 0), ("aɪ", 2), ("k", 0)])),
        ("dom", p(&["d", "ə", "m"])),
        ("ish", p(&["ɪ", "ʃ"])),
        ("able", p(&["ə", "b", "əl"])),
        ("ism", p(&["ɪ", "z", "ə", "m"])),
        ("ist", p(&["ɪ", "s", "t"])),
        ("wards", p(&["w", "ɚ", "d", "z"])),
        ("ward", p(&["w", "ɚ", "d"])),
        ("age", p(&["ɪ", "dʒ"])),
        ("ers", p(&["ɚ", "z"])),
        ("er", p(&["ɚ"])),
        ("est", p(&["ɪ", "s", "t"])),
        ("y", p(&["i"])),
    ]
}

/// Prefix spelling -> (form before a stem stressed on its first syllable, form otherwise).
type Forms = (Vec<Phone>, Vec<Phone>);

fn prefixes() -> Vec<(&'static str, Forms)> {
    let same = |v: Vec<Phone>| (v.clone(), v);
    vec![
        ("under", same(ph(&[("ʌ", 2), ("n", 0), ("d", 0), ("ɚ", 0)]))),
        ("over", same(ph(&[("oʊ", 2), ("v", 0), ("ɚ", 0)]))),
        ("anti", same(ph(&[("æ", 2), ("n", 0), ("t", 0), ("i", 0)]))),
        ("semi", same(ph(&[("s", 0), ("ɛ", 2), ("m", 0), ("i", 0)]))),
        ("non", same(ph(&[("n", 0), ("ɑː", 2), ("n", 0)]))),
        ("out", same(ph(&[("aʊ", 2), ("t", 0)]))),
        (
            "dis",
            (p(&["d", "ɪ", "s"]), ph(&[("d", 0), ("ɪ", 2), ("s", 0)])),
        ),
        (
            "mis",
            (p(&["m", "ɪ", "s"]), ph(&[("m", 0), ("ɪ", 2), ("s", 0)])),
        ),
        ("un", (p(&["ʌ", "n"]), ph(&[("ʌ", 2), ("n", 0)]))),
        ("in", (p(&["ɪ", "n"]), ph(&[("ɪ", 2), ("n", 0)]))),
        ("re", (p(&["ɹ", "ᵻ"]), ph(&[("ɹ", 0), ("iː", 2)]))),
        ("pre", same(ph(&[("p", 0), ("ɹ", 0), ("iː", 2)]))),
    ]
}

const BRITISH: [(&str, &str); 23] = [
    ("isation", "ization"),
    ("ising", "izing"),
    ("ised", "ized"),
    ("ises", "izes"),
    ("ise", "ize"),
    ("yse", "yze"),
    ("our", "or"),
    ("ourable", "orable"),
    ("ours", "ors"),
    ("oured", "ored"),
    ("ouring", "oring"),
    ("tre", "ter"),
    ("tres", "ters"),
    ("bre", "ber"),
    ("ence", "ense"),
    ("ogue", "og"),
    ("mme", "m"),
    ("lled", "led"),
    ("lling", "ling"),
    ("ller", "ler"),
    ("oul", "ol"),
    ("ae", "e"),
    ("enqui", "inqui"),
];

/// Stem + ending, adjusted at the seam.
pub fn join(stem: &[Phone], ending: &[Phone]) -> Vec<Phone> {
    let mut stem = stem.to_vec();
    if stem.is_empty() || ending.is_empty() {
        stem.extend(ending.iter().cloned());
        return stem;
    }
    let (last, stress) = stem[stem.len() - 1].clone();
    let (first, first_stress) = (ending[0].0.as_str(), ending[0].1);
    let n = stem.len();
    if is_vowel(first) {
        if let Some(v) = split_r(&last) {
            stem[n - 1] = (v.to_string(), stress);
            stem.push(("ɹ".into(), 0));
        } else if matches!(last.as_str(), "ɚ" | "ɜː" | "aɪɚ" | "ʊɹ") {
            stem.push(("ɹ".into(), 0));
        } else if last == "t"
            && n > 1
            && (is_vowel(&stem[n - 2].0) || stem[n - 2].0 == "ɹ")
            && first_stress == 0
            && !(first == "ə" && ending.len() > 1 && ending[1].0 == "n")
        {
            stem[n - 1] = ("ɾ".into(), 0);
        } else if last == "i" {
            stem[n - 1] = (if first != "ɪ" { "ɪ" } else { "i" }.to_string(), stress);
        }
    } else if last == "i" && matches!(first, "n" | "l" | "f" | "m") {
        stem[n - 1] = ("ɪ".into(), stress);
    } else if last == "n" && matches!(first, "k" | "ɡ") {
        stem[n - 1] = ("ŋ".into(), 0);
    }
    stem.extend(ending.iter().cloned());
    stem
}

pub fn plural(stem: &[Phone]) -> Vec<Phone> {
    let last = stem[stem.len() - 1].0.as_str();
    join(
        stem,
        &if SIBILANT.contains(&last) {
            p(&["ᵻ", "z"])
        } else if VOICELESS.contains(&last) {
            p(&["s"])
        } else {
            p(&["z"])
        },
    )
}

fn past(stem: &[Phone]) -> Vec<Phone> {
    let last = stem[stem.len() - 1].0.as_str();
    if matches!(last, "t" | "d" | "ɾ") {
        return join(stem, &p(&["ᵻ", "d"]));
    }
    join(
        stem,
        &if VOICELESS.contains(&last) || matches!(last, "s" | "ʃ" | "tʃ") {
            p(&["t"])
        } else {
            p(&["d"])
        },
    )
}

fn adverb(stem: &[Phone]) -> Vec<Phone> {
    let last = stem[stem.len() - 1].0.as_str();
    join(
        stem,
        &if matches!(last, "əl" | "l") {
            p(&["i"])
        } else {
            p(&["l", "i"])
        },
    )
}

/// `(?:^|[^aeiou])[aeiou](?:[bcdfgklmnprstvz]|th)$`: a short vowel before one consonant.
fn short_stem(body: &str) -> bool {
    let c: Vec<char> = body.chars().collect();
    let n = c.len();
    let vowel = |ch: char| "aeiou".contains(ch);
    let before_ok = |at: usize| at == 0 || !vowel(c[at - 1]);
    if n >= 2 && "bcdfgklmnprstvz".contains(c[n - 1]) && vowel(c[n - 2]) && before_ok(n - 2) {
        return true;
    }
    n >= 3 && c[n - 2] == 't' && c[n - 1] == 'h' && vowel(c[n - 3]) && before_ok(n - 3)
}

fn chop(word: &str, n: usize) -> String {
    let c: Vec<char> = word.chars().collect();
    c[..c.len().saturating_sub(n)].iter().collect()
}

/// Spellings the stem may have had before an ending starting with a vowel.
fn stems(body: &str) -> Vec<String> {
    let c: Vec<char> = body.chars().collect();
    let mut out = vec![body.to_string()];
    if c.len() > 2 && c[c.len() - 1] == c[c.len() - 2] && CONSONANT_LETTERS.contains(c[c.len() - 1])
    {
        out.push(chop(body, 1));
    }
    if let Some(&last) = c.last() {
        if CONSONANT_LETTERS.contains(last) || last == 'u' {
            let single = short_stem(body);
            let at = if single { 0 } else { out.len() };
            out.insert(at, format!("{body}e"));
        }
    }
    if body.ends_with('i') && c.len() > 2 {
        out.push(format!("{}y", chop(body, 1)));
    }
    out
}

fn secondary(phones: &[Phone]) -> Vec<Phone> {
    phones
        .iter()
        .map(|(x, s)| (x.clone(), if *s == 1 { 2 } else { *s }))
        .collect()
}

/// Two words said as one: the second keeps only a lesser stress ("sandpiper").
pub fn compound(first: &[Phone], second: &[Phone]) -> Vec<Phone> {
    join(first, &secondary(second))
}

fn stem_of(spelling: &str, lookup: Lookup, part: Lookup, depth: usize) -> Option<Vec<Phone>> {
    if spelling.chars().count() < 2 {
        return None;
    }
    lookup(spelling).or_else(|| {
        if depth < 2 {
            derive_at(spelling, lookup, part, depth + 1)
        } else {
            None
        }
    })
}

/// The first candidate with a stem decides: `build`'s answer is the answer.
fn first(
    candidates: &[String],
    lookup: Lookup,
    part: Lookup,
    depth: usize,
    build: &dyn Fn(Vec<Phone>) -> Option<Vec<Phone>>,
) -> Option<Vec<Phone>> {
    for spelling in candidates {
        if let Some(found) = stem_of(spelling, lookup, part, depth) {
            return build(found);
        }
    }
    None
}

/// Pronunciation of `word` from a shorter dictionary word, or `None`.
pub fn derive(word: &str, lookup: Lookup, part: Lookup) -> Option<Vec<Phone>> {
    derive_at(word, lookup, part, 0)
}

fn derive_at(word: &str, lookup: Lookup, part: Lookup, depth: usize) -> Option<Vec<Phone>> {
    let len = word.chars().count();
    let chars: Vec<char> = word.chars().collect();
    let some = |f: fn(&[Phone]) -> Vec<Phone>| move |found: Vec<Phone>| Some(f(&found));
    let try_first = |cands: Vec<String>, build: &dyn Fn(Vec<Phone>) -> Option<Vec<Phone>>| {
        first(&cands, lookup, part, depth, build)
    };

    if word.ends_with("'s") {
        return try_first(vec![chop(word, 2)], &some(plural));
    }
    if word.ends_with("s'") {
        return try_first(vec![chop(word, 1)], &Some);
    }
    if word.ends_with("ies") && len > 4 {
        if let Some(f) = try_first(vec![format!("{}y", chop(word, 3))], &some(plural)) {
            return Some(f);
        }
    }
    if word.ends_with("ves") && len > 4 {
        let build = |f: Vec<Phone>| {
            if f[f.len() - 1].0 == "f" {
                let mut head = f[..f.len() - 1].to_vec();
                head.push(("v".into(), 0));
                Some(join(&head, &p(&["z"])))
            } else {
                None
            }
        };
        if let Some(f) = try_first(
            vec![
                format!("{}f", chop(word, 3)),
                format!("{}fe", chop(word, 3)),
            ],
            &build,
        ) {
            return Some(f);
        }
    }
    if word.ends_with("es") && len > 3 {
        let cands = if "sxzh".contains(chars[len - 3]) {
            vec![chop(word, 2)]
        } else {
            vec![]
        };
        if let Some(f) = try_first(cands, &some(plural)) {
            return Some(f);
        }
    }
    if word.ends_with('s')
        && !(word.ends_with("ss") || word.ends_with("us") || word.ends_with("is"))
        && len > 3
    {
        if let Some(f) = try_first(vec![chop(word, 1)], &some(plural)) {
            return Some(f);
        }
    }
    if word.ends_with("ied") && len > 4 {
        if let Some(f) = try_first(vec![format!("{}y", chop(word, 3))], &some(past)) {
            return Some(f);
        }
    }
    if word.ends_with("ed") && len > 3 {
        if let Some(f) = try_first(stems(&chop(word, 2)), &some(past)) {
            return Some(f);
        }
    }
    if word.ends_with("ing") && len > 4 {
        let build = |f: Vec<Phone>| Some(join(&f, &p(&["ɪ", "ŋ"])));
        if let Some(f) = try_first(stems(&chop(word, 3)), &build) {
            return Some(f);
        }
    }
    if word.ends_with("ily") && len > 4 {
        if let Some(f) = try_first(vec![format!("{}y", chop(word, 3))], &some(adverb)) {
            return Some(f);
        }
    }
    if word.ends_with("ly") && len > 3 {
        let cands = if word.ends_with("bly") || word.ends_with("ply") {
            vec![
                chop(word, 2),
                format!("{}le", chop(word, 2)),
                format!("{}e", chop(word, 1)),
            ]
        } else {
            vec![chop(word, 2)]
        };
        if let Some(f) = try_first(cands, &some(adverb)) {
            return Some(f);
        }
    }
    for (spelling, phones) in endings() {
        let slen = spelling.chars().count();
        if word.ends_with(spelling) && len - slen > 2 {
            let body = chop(word, slen);
            if spelling == "y" && !CONSONANT_LETTERS.contains(body.chars().last().unwrap_or('a')) {
                continue;
            }
            let vowel_first = is_vowel(&phones[0].0);
            let cands = if vowel_first {
                stems(&body)
            } else if body.ends_with('i') {
                vec![format!("{}y", chop(&body, 1)), body.clone()]
            } else {
                vec![body.clone()]
            };
            let build = |f: Vec<Phone>| Some(join(&f, &phones));
            if let Some(f) = try_first(cands, &build) {
                return Some(f);
            }
        }
    }
    for (spelling, (plain, stressed)) in prefixes() {
        let slen = spelling.chars().count();
        if word.starts_with(spelling) && len - slen > 2 {
            let rest: String = chars[slen..].iter().collect();
            if let Some(found) = stem_of(&rest, lookup, part, depth) {
                let first_vowel_stress = found.iter().find(|(x, _)| is_vowel(x)).map(|(_, s)| *s);
                let head = if first_vowel_stress == Some(1) {
                    &plain
                } else {
                    &stressed
                };
                return Some(join(head, &found));
            }
        }
    }
    for (old, new) in BRITISH {
        if word.contains(old) {
            if let Some(found) = stem_of(&word.replacen(old, new, 1), lookup, part, depth) {
                return Some(found);
            }
        }
    }
    if depth == 0 {
        // Two dictionary words of four letters or more: "sandpiper", "houseplant".
        let mut best: Option<(usize, Vec<Phone>, Vec<Phone>)> = None;
        for cut in 4..len.saturating_sub(3) {
            let (a, b): (String, String) =
                (chars[..cut].iter().collect(), chars[cut..].iter().collect());
            if let (Some(left), Some(right)) = (part(&a), part(&b)) {
                let score = cut.min(len - cut);
                if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                    best = Some((score, left, right));
                }
            }
        }
        if let Some((_, left, right)) = best {
            return Some(compound(&left, &right));
        }
    }
    None
}
