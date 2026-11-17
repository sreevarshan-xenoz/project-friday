//! Hand-written tables (`data/*.txt`, a port of dizione's `tables.py`): plain
//! text, one entry per line, columns separated by tabs or by two or more
//! spaces, `#` starts a comment line.

use super::convert::{from_text, is_arpabet};
use super::symbols::Phone;
use std::collections::HashMap;

pub const FUNCTION_WORDS: &str = include_str!("data/function_words.txt");
pub const PHRASES: &str = include_str!("data/phrases.txt");
pub const EXCEPTIONS: &str = include_str!("data/exceptions.txt");
pub const LETTERS: &str = include_str!("data/letters.txt");
pub const HOMOGRAPH_RULES: &str = include_str!("data/homograph_rules.txt");

const STRESS_CLASSES: [&str; 8] = ["u", "uf", "ur", "s", "ss", "si", "pf", "p"];

/// Splits on `\t+| {2,}` the way the Python regex scans.
fn split_columns(line: &str) -> Vec<String> {
    let c: Vec<char> = line.chars().collect();
    let (mut out, mut cur, mut i) = (Vec::new(), String::new(), 0);
    while i < c.len() {
        if c[i] == '\t' {
            while i < c.len() && c[i] == '\t' {
                i += 1;
            }
            out.push(std::mem::take(&mut cur));
        } else if c[i] == ' ' && i + 1 < c.len() && c[i + 1] == ' ' {
            while i < c.len() && c[i] == ' ' {
                i += 1;
            }
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c[i]);
            i += 1;
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// Rows of a table: (line number, columns).
pub fn rows(text: &str) -> Vec<(usize, Vec<String>)> {
    text.lines()
        .enumerate()
        .filter_map(|(n, line)| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then(|| (n + 1, split_columns(line)))
        })
        .collect()
}

pub type FunctionWord = (String, Vec<Phone>, Option<Vec<Phone>>);

/// word -> (class, form, final form or None).
pub fn function_words() -> Result<HashMap<String, FunctionWord>, String> {
    let mut table = HashMap::new();
    for (number, c) in rows(FUNCTION_WORDS) {
        if !(c.len() == 3 || c.len() == 4) || !STRESS_CLASSES.contains(&c[1].as_str()) {
            return Err(format!("function_words.txt line {number}"));
        }
        let fin = if c.len() == 4 {
            Some(from_text(&c[3])?)
        } else {
            None
        };
        table.insert(c[0].clone(), (c[1].clone(), from_text(&c[2])?, fin));
    }
    Ok(table)
}

pub type Phrase = (String, Vec<Vec<Phone>>, Option<Vec<Phone>>);

/// (word, word, ...) -> (class, forms, final form or None).
pub fn phrases() -> Result<HashMap<Vec<String>, Phrase>, String> {
    let mut table = HashMap::new();
    for (number, c) in rows(PHRASES) {
        if !(c.len() == 3 || c.len() == 4) || !STRESS_CLASSES.contains(&c[1].as_str()) {
            return Err(format!("phrases.txt line {number}"));
        }
        let forms: Vec<Vec<Phone>> = c[2].split('|').map(from_text).collect::<Result<_, _>>()?;
        if forms.len() > 1 && (c[1] != "p" || forms.len() != c[0].split_whitespace().count()) {
            return Err(format!("phrases.txt line {number}: a form with boundaries needs class p and one part per word"));
        }
        let fin = if c.len() == 4 {
            Some(from_text(&c[3])?)
        } else {
            None
        };
        table.insert(
            c[0].split_whitespace().map(String::from).collect(),
            (c[1].clone(), forms, fin),
        );
    }
    Ok(table)
}

/// A pronunciation: CMUdict symbols, or phones of the inventory.
#[derive(Debug, Clone)]
pub enum Pron {
    Arpabet(Vec<String>),
    Phones(Vec<Phone>),
}

fn pronunciation(text: &str) -> Result<Pron, String> {
    let symbols: Vec<&str> = text.split_whitespace().collect();
    if is_arpabet(&symbols) {
        Ok(Pron::Arpabet(
            symbols.iter().map(|s| s.to_string()).collect(),
        ))
    } else {
        Ok(Pron::Phones(from_text(text)?))
    }
}

/// word -> pronunciation (single-word rows; several words are `names`).
pub fn pronunciations(text: &str) -> Result<HashMap<String, Pron>, String> {
    let mut table = HashMap::new();
    for (number, c) in rows(text) {
        if c.len() != 2 {
            return Err(format!(
                "line {number}: expected a word and its pronunciation"
            ));
        }
        if c[0].split_whitespace().count() == 1 {
            table.insert(c[0].to_lowercase(), pronunciation(&c[1])?);
        }
    }
    Ok(table)
}

/// (word, word, ...) -> one pronunciation per word, separated by `|`.
pub fn names(text: &str) -> Result<HashMap<Vec<String>, Vec<Pron>>, String> {
    let mut table = HashMap::new();
    for (number, c) in rows(text) {
        let words: Vec<String> = c[0]
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        if c.len() != 2 || words.len() < 2 {
            continue;
        }
        let parts: Vec<&str> = c[1].split('|').map(str::trim).collect();
        if parts.len() != words.len() {
            return Err(format!(
                "line {number}: {:?} has {} words and {} pronunciations",
                c[0],
                words.len(),
                parts.len()
            ));
        }
        table.insert(
            words,
            parts
                .iter()
                .map(|p| pronunciation(p))
                .collect::<Result<_, _>>()?,
        );
    }
    Ok(table)
}

/// Two-column table as a dictionary (second column may be empty).
pub fn mapping(text: &str) -> HashMap<String, String> {
    rows(text)
        .into_iter()
        .map(|(_, c)| (c[0].clone(), c.get(1).cloned().unwrap_or_default()))
        .collect()
}
