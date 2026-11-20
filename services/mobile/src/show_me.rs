//! `friday-ctl show "<thing>"`: where a control is in the focused window, so
//! FRIDAY Shell can draw a ring around it. The controls come from the
//! accessibility bus (the `controls` and `locate` UI verbs); this picks the
//! one the question names.

use serde::Deserialize;

/// A control as the `controls` verb lists it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Control {
    pub path: String,
    pub role: String,
    pub name: String,
}

const STOP: &[&str] = &[
    "where", "where's", "whereis", "is", "the", "a", "an", "setting", "settings", "option",
    "options", "for", "show", "me", "find", "how", "do", "i", "to", "change", "button", "my",
    "can", "in", "of", "on", "this", "that", "it", "its", "turn", "set", "go", "get", "menu",
    "page", "tab",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The question's words that could name a control.
pub fn terms(question: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words(question) {
        let w = w.trim_end_matches("'s").to_string();
        if w.len() >= 2 && !STOP.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// The control the question names, if one fits well: it holds at least half
/// of the question's words (each a whole word of its name, or the start of
/// one). Among equals: the name that is exactly the words, then the shortest.
pub fn pick<'a>(question: &str, controls: &'a [Control]) -> Option<&'a Control> {
    let terms = terms(question);
    if terms.is_empty() {
        return None;
    }
    let needed = terms.len().div_ceil(2);
    controls
        .iter()
        .filter_map(|c| {
            let name = words(&c.name);
            let hits = terms
                .iter()
                .filter(|t| {
                    name.iter()
                        .any(|n| n == *t || (t.len() >= 4 && n.starts_with(t.as_str())))
                })
                .count();
            let exact = name == terms;
            (hits >= needed).then_some((hits, exact, std::cmp::Reverse(name.len()), c))
        })
        .max_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)))
        .map(|(_, _, _, c)| c)
}

/// The names the model may pick from when none fits: list items can be
/// content (file names, email subjects), so only the other controls', at
/// most 80.
pub fn names_for_model(controls: &[Control]) -> Vec<&str> {
    let mut names: Vec<&str> = controls
        .iter()
        .filter(|c| c.role != "ListItem")
        .map(|c| c.name.as_str())
        .collect();
    names.dedup();
    names.truncate(80);
    names
}

/// The control with exactly this name (as the model gave it back).
pub fn named<'a>(name: &str, controls: &'a [Control]) -> Option<&'a Control> {
    let want = name.trim().trim_matches(['"', '\'', '.']).to_lowercase();
    controls
        .iter()
        .find(|c| c.name.trim().to_lowercase() == want)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Control {
        Control {
            path: format!("0/{}", name.len()),
            role: "PageTab".into(),
            name: name.into(),
        }
    }

    #[test]
    fn the_control_the_question_names_is_picked() {
        let cs = [
            c("Playback"),
            c("Output Devices"),
            c("Input Devices"),
            c("Configuration"),
        ];
        let got = |q: &str| pick(q, &cs).map(|c| c.name.as_str());
        assert_eq!(got("where's output devices?"), Some("Output Devices"));
        assert_eq!(got("show me the playback tab"), Some("Playback"));
        assert_eq!(got("where is the config page"), Some("Configuration"));
        assert_eq!(got("where's dark mode"), None);
        assert_eq!(got("where is it"), None);
    }

    #[test]
    fn exact_and_shorter_names_win_ties() {
        let cs = [c("Dark style options"), c("Dark style")];
        assert_eq!(pick("where's dark style", &cs).unwrap().name, "Dark style");
    }

    #[test]
    fn list_items_never_go_to_the_model() {
        let mut cs = vec![c("Appearance"), c("Display")];
        cs.push(Control {
            path: "0/9".into(),
            role: "ListItem".into(),
            name: "salary-2026.pdf".into(),
        });
        assert_eq!(names_for_model(&cs), ["Appearance", "Display"]);
    }

    #[test]
    fn the_models_answer_is_matched_by_name() {
        let cs = [c("Appearance"), c("Display")];
        assert_eq!(named(" \"appearance\". ", &cs).unwrap().name, "Appearance");
        assert!(named("Sound", &cs).is_none());
    }
}
