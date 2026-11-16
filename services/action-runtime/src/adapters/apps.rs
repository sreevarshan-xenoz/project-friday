//! Installed programs on Linux, as the desktop menu lists them: the `.desktop`
//! files in the standard application folders. The desktop loop uses this to
//! turn what a person says ("another terminal", "brave browser") into the
//! entry id `gtk-launch` takes, instead of guessing a program name.

use std::path::{Path, PathBuf};

/// An installed program: the id to launch (`foot`, `brave-browser`) and its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
}

/// The folders desktop entries live in (user, system, Flatpak), most specific first.
pub fn app_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = &data_home {
        dirs.push(d.join("applications"));
        dirs.push(d.join("flatpak/exports/share/applications"));
    }
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join("applications")),
    );
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

/// What the search looks at in one desktop entry.
#[derive(Default)]
struct Fields {
    name: String,
    generic: String,
    keywords: String,
    comment: String,
    categories: String,
}

fn parse(text: &str) -> Option<Fields> {
    let mut f = Fields::default();
    let (mut in_entry, mut is_app) = (false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Type" => is_app = value.trim() == "Application",
            "NoDisplay" | "Hidden" if value.trim() == "true" => return None,
            "Name" => f.name = value.trim().to_string(),
            "GenericName" => f.generic = value.trim().to_string(),
            "Keywords" => f.keywords = value.trim().to_string(),
            "Comment" => f.comment = value.trim().to_string(),
            "Categories" => f.categories = value.trim().to_string(),
            _ => {}
        }
    }
    (is_app && !f.name.is_empty()).then_some(f)
}

/// How well one entry matches the words of `query`: a name match counts most,
/// then the generic name, keywords, categories and the id; 0 is no match.
fn score(id: &str, f: &Fields, tokens: &[String]) -> u32 {
    let name = f.name.to_lowercase();
    let generic = f.generic.to_lowercase();
    let id = id.to_lowercase();
    let other = format!("{} {} {}", f.keywords, f.categories, f.comment).to_lowercase();
    let mut total = 0;
    for t in tokens {
        total += if name == *t || id == *t {
            6
        } else if name.starts_with(t.as_str()) || id.starts_with(t.as_str()) {
            4
        } else if name.contains(t.as_str()) || id.contains(t.as_str()) {
            3
        } else if generic.contains(t.as_str()) {
            2
        } else if other.contains(t.as_str()) {
            1
        } else {
            0
        };
    }
    total
}

/// The installed programs that best match `query`, best first, at most `limit`.
/// A program in more than one folder is listed once (the first folder wins).
pub fn find_apps(dirs: &[PathBuf], query: &str, limit: usize) -> Vec<AppEntry> {
    let tokens: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2 && !matches!(*t, "an" | "the" | "app" | "another" | "new" | "my"))
        .map(str::to_string)
        .collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    let mut hits: Vec<(u32, AppEntry)> = Vec::new();
    for dir in dirs {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = read
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
            .collect();
        files.sort();
        for path in files {
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let Some(fields) = std::fs::read_to_string(&path)
                .ok()
                .as_deref()
                .and_then(parse)
            else {
                continue;
            };
            let s = score(&id, &fields, &tokens);
            if s > 0 {
                hits.push((
                    s,
                    AppEntry {
                        id,
                        name: fields.name,
                    },
                ));
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    hits.into_iter().take(limit).map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(entries: &[(&str, &str)]) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "friday-apps-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, body) in entries {
            std::fs::write(dir.join(file), body).unwrap();
        }
        dir
    }

    const FOOT: &str = "[Desktop Entry]\nType=Application\nName=Foot\nGenericName=Terminal\nComment=A fast, lightweight terminal emulator\nCategories=System;TerminalEmulator;\nExec=foot\n";
    const BRAVE: &str = "[Desktop Entry]\nType=Application\nName=Brave Web Browser\nGenericName=Web Browser\nExec=brave %U\n";
    const FIREFOX: &str =
        "[Desktop Entry]\nType=Application\nName=Firefox\nGenericName=Web Browser\nExec=firefox\n";
    const HIDDEN: &str = "[Desktop Entry]\nType=Application\nName=Foot Server\nNoDisplay=true\n";

    #[test]
    fn a_description_finds_the_program_and_gives_the_entry_id() {
        let dir = menu(&[
            ("foot.desktop", FOOT),
            ("brave-browser.desktop", BRAVE),
            ("firefox.desktop", FIREFOX),
        ]);
        let ids = |q: &str| {
            find_apps(std::slice::from_ref(&dir), q, 5)
                .into_iter()
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("another terminal"), ["foot"]);
        assert_eq!(ids("terminal"), ["foot"]);
        assert_eq!(ids("brave browser")[0], "brave-browser");
        assert_eq!(ids("brave"), ["brave-browser"]);
        assert_eq!(ids("firefox"), ["firefox"]);
        // "browser" alone matches both web browsers (and nothing else).
        let mut browsers = ids("browser");
        browsers.sort();
        assert_eq!(browsers, ["brave-browser", "firefox"]);
        assert!(ids("spreadsheet").is_empty());
        assert!(ids("   ").is_empty());
    }

    #[test]
    fn hidden_entries_and_non_programs_are_left_out() {
        let dir = menu(&[
            ("foot.desktop", FOOT),
            ("footserver.desktop", HIDDEN),
            (
                "link.desktop",
                "[Desktop Entry]\nType=Link\nName=Foot docs\n",
            ),
            ("readme.txt", "Name=foot"),
        ]);
        let found = find_apps(&[dir], "foot", 5);
        assert_eq!(
            found,
            [AppEntry {
                id: "foot".into(),
                name: "Foot".into()
            }]
        );
    }

    #[test]
    fn the_first_folder_wins_and_the_limit_applies() {
        let user = menu(&[(
            "foot.desktop",
            "[Desktop Entry]\nType=Application\nName=My Foot\n",
        )]);
        let system = menu(&[
            ("foot.desktop", FOOT),
            ("foot2.desktop", FOOT),
            ("foot3.desktop", FOOT),
        ]);
        let found = find_apps(&[user, system], "foot", 2);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "My Foot");
    }
}
