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
    // A file saved by Windows tools often starts with a BOM.
    for line in notes.trim_start_matches('\u{feff}').lines() {
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
                // "1. x" is a list item; "1.5 GB is …" isn't.
                let rest = l[digits..].strip_prefix(['.', ')'])?;
                (digits > 0 && rest.starts_with(' ')).then_some(rest)
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
        // "**Style:**" heads a list of its own; one word (a rule like "---",
        // a line of a pasted key) isn't a fact; a key is never copied in.
        if f.len() < 3
            || f.len() > 300
            || f.trim_end_matches(['*', '_']).ends_with(':')
            || !f.contains(' ')
            || !f.chars().any(char::is_alphabetic)
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

/// Adds the fact to the file (once), keeping everything else in it; the
/// facts it replaced ("I moved to Pune" ends "I live in Chennai").
pub fn remember(fact: &str) -> std::io::Result<Vec<String>> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    remember_at(&path(), &[fact.to_string()], &today).map(|(_, closed)| closed)
}

/// The heading over facts that stopped being true; its lines (`* <fact>
/// (until <date>)`) are kept for the person, never sent with an ask.
const NO_LONGER: &str = "## No longer true";

/// What a fact is about, when a newer one can replace it: where the person
/// lives, where they work, or "my <thing> is …" (their editor, their name).
pub fn subject(fact: &str) -> Option<String> {
    let f = fact.trim().to_lowercase();
    let f = f.trim_start_matches("that ").trim();
    const LIVES: [&str; 7] = [
        "i live in ",
        "i moved to ",
        "i've moved to ",
        "i have moved to ",
        "i'm living in ",
        "i am living in ",
        "i'm based in ",
    ];
    if LIVES.iter().any(|p| f.starts_with(p)) || f.starts_with("i am based in ") {
        return Some("where I live".into());
    }
    const WORKS: [&str; 5] = [
        "i work at ",
        "i work for ",
        "i now work at ",
        "i now work for ",
        "i joined ",
    ];
    if WORKS.iter().any(|p| f.starts_with(p)) {
        return Some("where I work".into());
    }
    let rest = f.strip_prefix("my ")?;
    let (thing, _) = rest
        .split_once(" is ")
        .or_else(|| rest.split_once(" are "))?;
    let thing = thing.trim();
    (!thing.is_empty() && thing.split_whitespace().count() <= 4).then(|| format!("my {thing}"))
}

/// Adds each fact the file doesn't have yet; how many it added.
pub fn remember_all(new: &[String]) -> std::io::Result<usize> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    remember_at(&path(), new, &today).map(|(n, _)| n)
}

/// What the file says now: empty when there's none yet, an error when it is
/// there but can't be read (so it is never written over).
pub fn read(p: &std::path::Path) -> std::io::Result<String> {
    match std::fs::read_to_string(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        r => r,
    }
}

/// Whether these facts would no longer all fit in what an ask carries.
pub fn too_long(all: &[String]) -> bool {
    all.iter().map(|f| f.len() + 2).sum::<usize>() + "about you: ".len() > MAX_CHARS
}

/// Adds each new fact; one with the same [`subject`] as a fact already there
/// moves that one under [`NO_LONGER`], dated `today` (with `memory-changes`
/// on). How many it added, and the facts it closed.
fn remember_at(
    p: &std::path::Path,
    new: &[String],
    today: &str,
) -> std::io::Result<(usize, Vec<String>)> {
    let text = read(p)?;
    let mut lines: Vec<String> = if text.trim().is_empty() {
        vec!["# What FRIDAY remembers about you. Edit freely: one \"- \" line each.".to_string()]
    } else {
        text.lines().map(str::to_string).collect()
    };
    let changes = crate::features::on("memory-changes");
    let (mut added, mut closed) = (0, Vec::new());
    for fact in new {
        if facts(&lines.join("\n"))
            .iter()
            .any(|f| f.eq_ignore_ascii_case(fact))
        {
            continue;
        }
        if let Some(key) = subject(fact).filter(|_| changes) {
            let old: Vec<usize> = (0..lines.len())
                .filter(|&i| {
                    lines[i]
                        .trim()
                        .strip_prefix("- ")
                        .and_then(subject)
                        .is_some_and(|k| k == key)
                })
                .collect();
            for &i in old.iter().rev() {
                let was = lines
                    .remove(i)
                    .trim()
                    .trim_start_matches("- ")
                    .trim()
                    .to_string();
                if !lines.iter().any(|l| l.trim() == NO_LONGER) {
                    if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                        lines.push(String::new());
                    }
                    lines.push(NO_LONGER.to_string());
                }
                lines.push(format!("* {was} (until {today})"));
                closed.push(was);
            }
        }
        // Current facts stay above the closed ones.
        let at = lines
            .iter()
            .position(|l| l.trim() == NO_LONGER)
            .map(|i| {
                let mut i = i;
                while i > 0 && lines[i - 1].trim().is_empty() {
                    i -= 1;
                }
                i
            })
            .unwrap_or(lines.len());
        lines.insert(at, format!("- {fact}"));
        added += 1;
    }
    if added == 0 {
        return Ok((0, closed));
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("md.tmp");
    std::fs::write(&tmp, lines.join("\n") + "\n")?;
    std::fs::rename(tmp, p)?;
    Ok((added, closed))
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
        let notes = "# CLAUDE.md\n\nSome intro prose.\n\n## Style\n- Use tabs, not spaces\n* I prefer short answers\n1. My editor is Helix\n- [x] Tests run with cargo nextest\n- Rules:\n  - Nested is fine too\n```\n- not this, it's code\n```\n| a | b |\n- I use fish\n- token: abcdefghijklmnopqrstuvwxyz0123456789ABCD\n- use tabs, NOT spaces\n- **Style:**\n---\n2) 3.11 is my Python\n";
        let have = vec!["I use fish".to_string()];
        assert_eq!(
            importable(notes, &have),
            vec![
                "Use tabs, not spaces",
                "I prefer short answers",
                "My editor is Helix",
                "Tests run with cargo nextest",
                "Nested is fine too",
                "3.11 is my Python",
            ]
        );
        // A plain export (one memory a line) has no list: each line is one.
        assert_eq!(
            importable(
                "\u{feff}Lives in Chennai.\n---\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA\n\nPrefers dark mode.\n",
                &[]
            ),
            vec!["Lives in Chennai.", "Prefers dark mode."]
        );
        assert!(importable("```\nfn main() {}\n```\n# Title\n", &[]).is_empty());
        assert_eq!(
            importable("\u{feff}- I live here\n", &[]),
            vec!["I live here"]
        );
        assert_eq!(
            importable("1.5 GB is all the RAM I have\n", &[]),
            vec!["1.5 GB is all the RAM I have"]
        );
    }

    #[test]
    fn remember_all_adds_only_what_is_new() {
        // Its own file: other tests write path() at the same time.
        let p = std::env::temp_dir().join(format!("friday-test-import-{}.md", std::process::id()));
        let _ = std::fs::remove_file(&p);
        assert_eq!(
            remember_at(
                &p,
                &["a fact".into(), "A FACT".into(), "two".into()],
                "2026-11-23"
            )
            .unwrap()
            .0,
            2
        );
        assert_eq!(remember_at(&p, &["two".into()], "2026-11-23").unwrap().0, 0);
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(facts(&text), vec!["a fact".to_string(), "two".to_string()]);
        assert!(text.starts_with("# What FRIDAY remembers"));
        // A file that can't be read is never written over.
        let odd = [0xff, 0xfe, b'-', 0, b' ', 0];
        std::fs::write(&p, odd).unwrap();
        assert!(remember_at(&p, &["three".into()], "2026-11-23").is_err());
        assert_eq!(std::fs::read(&p).unwrap(), odd);
        let _ = std::fs::remove_file(&p);
        assert!(!too_long(&["short".into()]));
        assert!(too_long(&vec!["x".repeat(100); 20]));
    }

    #[test]
    fn a_changed_fact_closes_the_old_one_with_a_date() {
        assert_eq!(subject("I moved to Pune"), subject("I live in Chennai"));
        assert_eq!(subject("my editor is Helix").as_deref(), Some("my editor"));
        assert_eq!(subject("I use fish"), None);
        assert_ne!(subject("my editor is Helix"), subject("my shell is fish"));
        let p = std::env::temp_dir().join(format!("friday-test-change-{}.md", std::process::id()));
        std::fs::write(
            &p,
            "# mine\n- I live in Chennai\n- my editor is Helix\n- I use fish\n",
        )
        .unwrap();
        let (n, closed) = remember_at(&p, &["I moved to Pune".into()], "2026-11-23").unwrap();
        assert_eq!((n, closed), (1, vec!["I live in Chennai".to_string()]));
        remember_at(&p, &["my editor is Zed".into()], "2026-12-01").unwrap();
        remember_at(&p, &["I have a cat".into()], "2026-12-02").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        // Only what is true now goes with an ask; the old facts stay, dated.
        assert_eq!(
            facts(&text),
            vec![
                "I use fish",
                "I moved to Pune",
                "my editor is Zed",
                "I have a cat"
            ]
        );
        assert!(text.contains("## No longer true\n* I live in Chennai (until 2026-11-23)\n* my editor is Helix (until 2026-12-01)\n"), "{text}");
        assert!(
            text.find("I have a cat").unwrap() < text.find(NO_LONGER).unwrap(),
            "{text}"
        );
        let _ = std::fs::remove_file(&p);
    }
}
