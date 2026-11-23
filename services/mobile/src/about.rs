//! What the person asked FRIDAY to remember about them, in one plain file
//! they can read and edit: `memory.md` in FRIDAY's config folder
//! (`~/.config/friday`, `%APPDATA%\friday`), a line `- <fact>` each. "Remember
//! that I use fish" adds one; every ask carries them (`about you:` in
//! `[memory]`); `friday memory forget` or an edit takes one out.

use std::path::PathBuf;

/// The most of the file an ask carries.
const MAX_CHARS: usize = 1500;

pub fn path() -> PathBuf {
    // Tests never touch the person's own file.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("friday-test-memory-{}.md", std::process::id()));
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_default().join("friday").join("memory.md")
}

/// The facts in the file's text: its `- ` lines (anything else, such as a
/// heading or a note to self, is the person's own and skipped).
pub fn facts(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("- ").map(str::trim))
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect()
}

/// The line an ask carries, at most `MAX_CHARS`, secrets masked, or None
/// with nothing kept (or `memory-file` off).
pub fn context_line() -> Option<String> {
    if !crate::features::on("memory-file") {
        return None;
    }
    let all = facts(&std::fs::read_to_string(path()).ok()?);
    let mut line = String::from("about you: ");
    for f in all {
        if line.len() + f.len() + 2 > MAX_CHARS {
            continue;
        }
        if !line.ends_with(": ") {
            line.push_str("; ");
        }
        line.push_str(&f);
    }
    // A hand-written line might hold a key: never sent as typed.
    (!line.ends_with(": ")).then(|| friday_goal_execution::mask_secrets(&line).0)
}

/// The fact a "remember that …" request names, in the person's words.
pub fn fact_in(request: &str) -> Option<String> {
    let r = request.trim();
    let lower = r.to_lowercase();
    // Only "remember that …": "remember to buy milk" or "remember when we
    // fixed it?" aren't facts about the person.
    let rest = ["remember that ", "remember: "]
        .iter()
        .find(|p| lower.starts_with(*p))
        .map(|p| r[p.len()..].trim())?;
    let fact = rest.trim_end_matches(['.', '!']).trim();
    (!fact.is_empty() && fact.len() <= 300 && !fact.ends_with('?')).then(|| fact.to_string())
}

/// The facts another assistant's notes hold (a CLAUDE.md, an exported memory
/// list): their list lines, or every plain line when there is no list. Not
/// headings, code, tables, long lines, anything holding a secret, nor what
/// `have` already says.
pub fn importable(notes: &str, have: &[String]) -> Vec<String> {
    let (mut listed, mut plain) = (Vec::new(), Vec::new());
    let mut in_code = false;
    for line in notes.lines() {
        let l = line.trim();
        if l.starts_with("```") || l.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code || l.is_empty() || l.starts_with(['#', '|', '<', '>']) {
            continue;
        }
        let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let item = ["- ", "* ", "+ "]
            .iter()
            .find_map(|b| l.strip_prefix(b))
            .or_else(|| {
                (digits > 0)
                    .then(|| l[digits..].strip_prefix(['.', ')']))
                    .flatten()
            });
        match item {
            Some(rest) => {
                let rest = rest.trim();
                let rest = ["[ ] ", "[x] ", "[X] "]
                    .iter()
                    .find_map(|c| rest.strip_prefix(c))
                    .unwrap_or(rest);
                listed.push(rest.trim().to_string())
            }
            None => plain.push(l.to_string()),
        }
    }
    let mut out: Vec<String> = Vec::new();
    for f in if listed.is_empty() { plain } else { listed } {
        // "Style:" heads a list of its own; a key is never copied in.
        if f.len() < 3
            || f.len() > 300
            || f.ends_with(':')
            || friday_goal_execution::mask_secrets(&f).0 != f
        {
            continue;
        }
        if !have.iter().chain(&out).any(|h| h.eq_ignore_ascii_case(&f)) {
            out.push(f);
        }
    }
    out
}

/// Adds the fact to the file (once), keeping everything else in it.
pub fn remember(fact: &str) -> std::io::Result<()> {
    remember_all(&[fact.to_string()]).map(|_| ())
}

/// Adds each fact the file doesn't have yet; how many it added.
pub fn remember_all(new: &[String]) -> std::io::Result<usize> {
    let p = path();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let mut have = facts(&text);
    let mut out = if text.trim().is_empty() {
        "# What FRIDAY remembers about you. Edit freely: one \"- \" line each.\n".to_string()
    } else {
        text.clone()
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let mut added = 0;
    for fact in new {
        if have.iter().any(|f| f.eq_ignore_ascii_case(fact)) {
            continue;
        }
        out.push_str(&format!("- {fact}\n"));
        have.push(fact.clone());
        added += 1;
    }
    if added == 0 {
        return Ok(0);
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("md.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, p)?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_are_the_dash_lines_and_a_request_names_one() {
        assert_eq!(
            facts("# mine\n- I use fish\nnote to self\n-  my editor is Helix \n- \n"),
            vec!["I use fish".to_string(), "my editor is Helix".to_string()]
        );
        assert_eq!(
            fact_in("Remember that I use fish."),
            Some("I use fish".into())
        );
        assert_eq!(
            fact_in("remember: my name is Ada"),
            Some("my name is Ada".into())
        );
        assert_eq!(fact_in("what do you remember"), None);
        assert_eq!(fact_in("remember to buy milk"), None);
        assert_eq!(fact_in("remember that we fixed it?"), None);
    }

    #[test]
    fn notes_from_another_assistant_bring_in_their_list_lines_only() {
        let notes = "# CLAUDE.md\n\nSome intro prose.\n\n## Style\n- Use tabs, not spaces\n* I prefer short answers\n1. My editor is Helix\n- [x] Tests run with cargo nextest\n- Rules:\n  - Nested is fine too\n```\n- not this, it's code\n```\n| a | b |\n- I use fish\n- token: abcdefghijklmnopqrstuvwxyz0123456789ABCD\n- use tabs, NOT spaces\n";
        let have = vec!["I use fish".to_string()];
        assert_eq!(
            importable(notes, &have),
            vec![
                "Use tabs, not spaces",
                "I prefer short answers",
                "My editor is Helix",
                "Tests run with cargo nextest",
                "Nested is fine too",
            ]
        );
        // A plain export (one memory a line) has no list: each line is one.
        assert_eq!(
            importable("Lives in Chennai.\n\nPrefers dark mode.\n", &[]),
            vec!["Lives in Chennai.", "Prefers dark mode."]
        );
        assert!(importable("```\nfn main() {}\n```\n# Title\n", &[]).is_empty());
    }

    #[test]
    fn remember_all_adds_only_what_is_new() {
        let _ = std::fs::remove_file(path());
        assert_eq!(
            remember_all(&["a fact".into(), "A FACT".into(), "two".into()]).unwrap(),
            2
        );
        assert_eq!(remember_all(&["two".into()]).unwrap(), 0);
        let text = std::fs::read_to_string(path()).unwrap();
        assert_eq!(facts(&text), vec!["a fact".to_string(), "two".to_string()]);
        assert!(text.starts_with("# What FRIDAY remembers"));
        let _ = std::fs::remove_file(path());
    }
}
