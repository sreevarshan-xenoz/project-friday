//! `friday memory forget <words>` forgets everywhere a fact was copied, not
//! only its line in memory.md: the memory journal, conversations, lessons,
//! day summaries, the screen recall index, the answer and ask history, and a
//! pending tidy proposal. Every record holding the words (any case) goes;
//! the rest stays. The person's own thoughts are notes, not copies, and stay.

use std::path::{Path, PathBuf};

/// Where the copies live.
pub struct Places {
    /// FRIDAY's state folder (beside state.json).
    pub state: PathBuf,
    /// The memory journal's folder (episodes.jsonl).
    pub memory: PathBuf,
    /// memory.md's tidy proposal.
    pub proposal: PathBuf,
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("forget.tmp");
    std::fs::write(&tmp, text)?;
    // The file keeps its permissions (records are readable only by you).
    if let Ok(m) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, m.permissions());
    }
    std::fs::rename(&tmp, path)
}

/// Whether `hay` (lowercase) has `w` as whole words: "the" isn't in "weather".
fn has_words(hay: &str, w: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    hay.match_indices(w).any(|(at, _)| {
        !word(hay[..at].chars().next_back()) && !word(hay[at + w.len()..].chars().next())
    })
}

/// A record's own words: a JSON line's string values (never its keys), or
/// the line itself.
fn holds(line: &str, w: &str) -> bool {
    fn values(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::String(s) => {
                out.push_str(&s.to_lowercase());
                out.push('\n');
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| values(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| values(x, out)),
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) if v.is_object() || v.is_array() => {
            let mut text = String::new();
            values(&v, &mut text);
            has_words(&text, w)
        }
        _ => has_words(&line.to_lowercase(), w),
    }
}

/// `text` without its lines holding `w`; how many went.
fn drop_lines(text: &str, w: &str) -> (String, usize) {
    let mut gone = 0;
    let mut out = String::new();
    for l in text.lines() {
        if holds(l, w) {
            gone += 1;
        } else {
            out.push_str(l);
            out.push('\n');
        }
    }
    (out, gone)
}

fn lines_in(path: &Path, w: &str) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    let (kept, gone) = drop_lines(&text, w);
    if gone > 0 && write(path, &kept).is_err() {
        return 0;
    }
    gone
}

/// Removes every copy of `words` from `places`. Returns (what, how many)
/// for each place where something went.
pub fn everywhere(places: &Places, words: &str) -> Result<Vec<(&'static str, usize)>, String> {
    let w = words.trim().to_lowercase();
    if w.chars().count() < 3 {
        return Err("name at least three letters of what to forget".into());
    }
    let s = &places.state;
    let mut done: Vec<(&'static str, usize)> = Vec::new();
    let mut add = |what, n: usize| {
        if n > 0 {
            done.push((what, n));
        }
    };
    add(
        "memory journal",
        lines_in(&places.memory.join("episodes.jsonl"), &w),
    );
    add("lessons", lines_in(&s.join("lessons.jsonl"), &w));
    add("ask history", lines_in(&s.join("ask-history"), &w));
    // Conversations: a whole conversation that held it.
    let conv = s.join("conversations.json");
    if let Ok(text) = std::fs::read_to_string(&conv) {
        if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str(&text) {
            let before = map.len();
            map.retain(|_, v| !holds(&v.to_string(), &w));
            if map.len() < before
                && write(&conv, &serde_json::Value::Object(map.clone()).to_string()).is_ok()
            {
                add("conversations", before - map.len());
            }
        }
    }
    // Day summaries and the screen recall index: their lines.
    for (dir, what) in [
        (s.join("days"), "day summaries"),
        (s.join("recall"), "screen recall"),
    ] {
        // An older FRIDAY's single recall file too.
        let mut n = if what == "screen recall" {
            lines_in(&s.join("recall.jsonl"), &w)
        } else {
            0
        };
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.path().is_file() {
                n += lines_in(&e.path(), &w);
            }
        }
        add(what, n);
    }
    // Answer history: each answer is a block; one that held it goes whole.
    for name in ["answers.md", "answers.1.md"] {
        let p = s.join(name);
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        const MARK: &str = "<!-- friday answer -->";
        let blocks: Vec<&str> = text.split(MARK).collect();
        let kept: Vec<&str> = blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| *i == 0 || !has_words(&b.to_lowercase(), &w))
            .map(|(_, b)| *b)
            .collect();
        // What comes before the first answer is checked too.
        let head_has = has_words(&blocks[0].to_lowercase(), &w);
        let mut kept = kept;
        if head_has {
            kept[0] = "";
        }
        let gone = blocks.len() - kept.len() + usize::from(head_has);
        if gone > 0 && write(&p, &kept.join(MARK)).is_ok() {
            add("answers", gone);
        }
    }
    // A pending tidy proposal that still has it: gone whole.
    if std::fs::read_to_string(&places.proposal).is_ok_and(|t| t.to_lowercase().contains(&w))
        && std::fs::remove_file(&places.proposal).is_ok()
    {
        add("memory tidy proposal", 1);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forgotten_fact_goes_from_every_copy_and_nothing_else_does() {
        let base = std::env::temp_dir().join(format!("friday-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (s, m) = (base.join("state"), base.join("state/memory"));
        for d in [&m, &s.join("days"), &s.join("recall")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let fact = "I live in Pune";
        std::fs::write(
            m.join("episodes.jsonl"),
            format!("{{\"q\":\"{fact}\"}}\n{{\"q\":\"other\"}}\n"),
        )
        .unwrap();
        std::fs::write(
            s.join("lessons.jsonl"),
            "{\"l\":\"pune weather\"}\n{\"l\":\"keep\"}\n",
        )
        .unwrap();
        std::fs::write(s.join("ask-history"), "where do I live, pune?\nwhat time\n").unwrap();
        std::fs::write(
            s.join("conversations.json"),
            r#"{"a":{"turns":["I live in Pune"]},"b":{"turns":["hi"]}}"#,
        )
        .unwrap();
        std::fs::write(
            s.join("days/2026-11-20.md"),
            "- asked about Pune rent\n- fixed a bug\n",
        )
        .unwrap();
        std::fs::write(
            s.join("recall/123.jsonl"),
            "{\"text\":\"Pune map\"}\n{\"text\":\"code\"}\n",
        )
        .unwrap();
        std::fs::write(
            s.join("answers.md"),
            "<!-- friday answer -->\nYou live in Pune.\n---\n<!-- friday answer -->\nThe build passed.\n---\n",
        )
        .unwrap();
        std::fs::write(
            s.join("thoughts.jsonl"),
            "{\"text\":\"move out of Pune\"}\n",
        )
        .unwrap();
        let proposal = base.join("memory-tidy.md");
        std::fs::write(&proposal, "- Lives in Pune\n").unwrap();
        let places = Places {
            state: s.clone(),
            memory: m.clone(),
            proposal: proposal.clone(),
        };

        assert!(everywhere(&places, "pu").is_err(), "too short to be safe");
        // A JSON key or part of a longer word is never a match.
        assert!(everywhere(&places, "text").unwrap().is_empty());
        assert!(!holds("{\"q\":\"weather\"}", "the"));
        let done = everywhere(&places, "PUNE").unwrap();
        assert_eq!(done.len(), 8, "{done:?}");
        let all = |p: PathBuf| {
            std::fs::read_to_string(p)
                .unwrap_or_default()
                .to_lowercase()
        };
        for p in [
            m.join("episodes.jsonl"),
            s.join("lessons.jsonl"),
            s.join("ask-history"),
            s.join("conversations.json"),
            s.join("days/2026-11-20.md"),
            s.join("recall/123.jsonl"),
            s.join("answers.md"),
        ] {
            assert!(
                !all(p.clone()).contains("pune"),
                "{} still has it",
                p.display()
            );
        }
        assert!(all(m.join("episodes.jsonl")).contains("other"));
        assert!(all(s.join("days/2026-11-20.md")).contains("fixed a bug"));
        assert!(all(s.join("answers.md")).contains("the build passed"));
        assert!(all(s.join("conversations.json")).contains("\"b\""));
        assert!(!proposal.exists());
        assert!(
            all(s.join("thoughts.jsonl")).contains("pune"),
            "your own notes stay"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
