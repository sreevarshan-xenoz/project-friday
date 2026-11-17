//! Sentence rules: what a word's pronunciation owes to its neighbours (a port
//! of dizione's `sentence.py`): sentence stress, "the"/"to" before a vowel,
//! linking r and the flap at the end of "it" and "at".

use super::symbols::{is_vowel, Phone};

#[derive(Debug, Clone)]
pub struct Unit {
    /// Spelling in lower case; several words for a phrase.
    pub word: String,
    pub phones: Vec<Phone>,
    /// Stress class of function_words.txt; "c" = stress as given.
    pub kind: String,
    /// Form to use when the word is promoted.
    pub final_form: Option<Vec<Phone>>,
    /// No word boundary before this unit (hyphenated compound).
    pub attach: bool,
}

impl Unit {
    pub fn new(
        word: &str,
        phones: Vec<Phone>,
        kind: &str,
        final_form: Option<Vec<Phone>>,
        attach: bool,
    ) -> Self {
        Self {
            word: word.to_string(),
            phones,
            kind: kind.to_string(),
            final_form,
            attach,
        }
    }
    fn last_word(&self) -> &str {
        self.word.rsplit(' ').next().unwrap_or("")
    }
    fn first_word(&self) -> &str {
        self.word.split(' ').next().unwrap_or("")
    }
}

fn split_r(phone: &str) -> Option<&'static str> {
    Some(match phone {
        "ɑːɹ" => "ɑː",
        "ɔːɹ" => "ɔː",
        "oːɹ" => "oː",
        "ɪɹ" => "ɪ",
        "ɛɹ" => "ɛ",
        "ʊɹ" => "ʊ",
        _ => return None,
    })
}

pub fn unstress(phones: &[Phone]) -> Vec<Phone> {
    phones.iter().map(|(p, _)| (p.clone(), 0)).collect()
}

/// Gives the word a primary stress: on its marked vowel, else on the first vowel.
pub fn promote(phones: &[Phone]) -> Vec<Phone> {
    let marks: Vec<u8> = phones.iter().map(|(_, s)| *s).collect();
    if marks.contains(&1) {
        return phones.to_vec();
    }
    let target = if marks.contains(&2) {
        marks.iter().rposition(|&m| m == 2)
    } else {
        phones.iter().position(|(p, _)| is_vowel(p))
    };
    phones
        .iter()
        .enumerate()
        .map(|(i, (p, s))| (p.clone(), if Some(i) == target { 1 } else { *s }))
        .collect()
}

/// Resolves the stress class of every unit of a clause, right to left.
pub fn stress_clause(units: &mut [Unit]) {
    let mut primary_follows = false;
    for index in (0..units.len()).rev() {
        let last = index + 1 == units.len();
        let unit = &units[index];
        let promoted = || match &unit.final_form {
            Some(f) => f.clone(),
            None => promote(&unit.phones),
        };
        let phones = match unit.kind.as_str() {
            "u" => unstress(&unit.phones),
            "uf" => {
                if last {
                    promoted()
                } else {
                    unstress(&unit.phones)
                }
            }
            "ur" => {
                if primary_follows {
                    unstress(&unit.phones)
                } else {
                    promoted()
                }
            }
            "s" => {
                if primary_follows {
                    unit.phones.clone()
                } else {
                    promoted()
                }
            }
            "si" => {
                if !primary_follows {
                    promoted()
                } else if index == 0 {
                    unit.phones.clone()
                } else {
                    unstress(&unit.phones)
                }
            }
            "pf" => {
                if last {
                    promoted()
                } else {
                    unit.phones.clone()
                }
            }
            _ => unit.phones.clone(),
        };
        units[index].phones = phones;
        primary_follows = primary_follows || units[index].phones.iter().any(|(_, s)| *s == 1);
    }
    if !units.is_empty() && !units.iter().any(|u| u.phones.iter().any(|(_, s)| *s != 0)) {
        // A clause is never without any stress.
        for unit in units.iter_mut().rev() {
            if unit.phones.iter().any(|(p, _)| is_vowel(p)) {
                unit.phones = promote(&unit.phones);
                break;
            }
        }
    }
}

/// Adjusts the end of each word to the beginning of the next.
pub fn join_clause(units: &mut [Unit]) {
    for k in 0..units.len().saturating_sub(1) {
        let (head, tail) = units.split_at_mut(k + 1);
        let (unit, following) = (&mut head[k], &tail[0]);
        if unit.phones.is_empty()
            || following.phones.is_empty()
            || !is_vowel(&following.phones[0].0)
        {
            continue;
        }
        let (last, stress) = unit.phones[unit.phones.len() - 1].clone();
        let n = unit.phones.len();
        if unit.last_word() == "the" && last == "ə" {
            unit.phones[n - 1] = ("ɪ".into(), stress);
        } else if unit.last_word() == "to" && last == "ə" {
            unit.phones[n - 1] = ("ʊ".into(), stress);
        } else if matches!(following.first_word(), "and" | "or" | "if") {
            continue;
        } else if let Some(vowel) = split_r(&last) {
            unit.phones[n - 1] = (vowel.into(), stress);
            unit.phones.push(("ɹ".into(), 0));
        } else if matches!(last.as_str(), "ɚ" | "ɜː" | "aɪɚ") {
            unit.phones.push(("ɹ".into(), 0));
        } else if last == "t"
            && matches!(unit.word.as_str(), "it" | "at")
            && n > 1
            && is_vowel(&unit.phones[n - 2].0)
        {
            unit.phones[n - 1] = ("ɾ".into(), 0);
        }
    }
}

/// Units of a clause as spoken words: an attached unit continues the one before.
pub fn groups(units: &[Unit]) -> Vec<Vec<Phone>> {
    let mut out: Vec<Vec<Phone>> = Vec::new();
    for unit in units {
        if unit.attach && !out.is_empty() {
            out.last_mut().unwrap().extend(unit.phones.iter().cloned());
        } else {
            out.push(unit.phones.clone());
        }
    }
    out.into_iter().filter(|g| !g.is_empty()).collect()
}
