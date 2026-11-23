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

/// The line an ask carries, at most `MAX_CHARS`, or None with nothing kept.
pub fn context_line() -> Option<String> {
    let all = facts(&std::fs::read_to_string(path()).ok()?);
    let mut line = String::from("about you: ");
    for f in all {
        if line.len() + f.len() + 2 > MAX_CHARS {
            break;
        }
        if !line.ends_with(": ") {
            line.push_str("; ");
        }
        line.push_str(&f);
    }
    (!line.ends_with(": ")).then_some(line)
}

/// The fact a "remember that …" request names, in the person's words.
pub fn fact_in(request: &str) -> Option<String> {
    let r = request.trim();
    let lower = r.to_lowercase();
    let rest = ["remember that ", "remember: ", "remember "]
        .iter()
        .find(|p| lower.starts_with(*p))
        .map(|p| r[p.len()..].trim())?;
    let fact = rest.trim_end_matches(['.', '!']).trim();
    (!fact.is_empty() && fact.len() <= 300).then(|| fact.to_string())
}

/// Adds the fact to the file (once), keeping everything else in it.
pub fn remember(fact: &str) -> std::io::Result<()> {
    let p = path();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    if facts(&text).iter().any(|f| f.eq_ignore_ascii_case(fact)) {
        return Ok(());
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = if text.trim().is_empty() {
        "# What FRIDAY remembers about you. Edit freely: one \"- \" line each.\n".to_string()
    } else {
        text.clone()
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("- {fact}\n"));
    let tmp = p.with_extension("md.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, p)
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
    }
}
