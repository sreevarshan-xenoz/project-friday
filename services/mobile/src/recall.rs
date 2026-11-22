//! "What was that?": an opt-in memory of the text of the windows you looked
//! at, kept on this computer for three hours (or 24 hours or 7 days, as
//! `friday recall keep` chose: `recall-keep` beside the file, in hours). `friday recall on` starts
//! `friday-ctl recall watch` (the `friday-recall` user service); every 30
//! seconds it keeps the text of the window in front when it changed (and
//! nothing while the screen is locked). Private windows (matched by whole
//! words) are never kept, nor are password fields (the reader leaves them
//! out). No screenshots. `friday-ctl recall search "<question>"` finds the
//! best passages for a question; `friday recall off` stops and forgets.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::local_docs::{self, Excerpt, TITLE_MARK};

/// How long text is kept, in seconds, unless `recall-keep` says otherwise.
pub const KEEP_SECS: i64 = 3 * 60 * 60;
/// The longer keeps a person can choose, in hours.
pub const KEEP_HOURS: [i64; 3] = [3, 24, 7 * 24];

/// How long the looks in `path` are kept: `recall-keep` beside it (3, 24 or
/// 168 hours), else three hours.
pub fn keep_secs(path: &Path) -> i64 {
    std::fs::read_to_string(path.with_file_name("recall-keep"))
        .ok()
        .and_then(|t| t.trim().parse::<i64>().ok())
        .filter(|h| KEEP_HOURS.contains(h))
        .map_or(KEEP_SECS, |h| h * 60 * 60)
}
/// Seconds between looks.
pub const EVERY_SECS: u64 = 30;
/// The most text kept, in bytes (a week of looks can add up): the oldest
/// hours go first.
pub const MAX_BYTES: usize = 20_000_000;
/// Looks are kept in one file per hour (`recall/<hour>.jsonl` beside
/// `path`), appended to and dropped whole, so a week of them is never
/// rewritten every 30 seconds.
const HOUR: i64 = 3600;

/// One look at a window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Seen {
    /// Unix seconds.
    pub ts: i64,
    pub app: String,
    pub title: String,
    pub text: String,
}

/// Where the text is kept: `recall.jsonl` in FRIDAY's state directory (the
/// hourly files go in `recall/` beside it; an older FRIDAY's single
/// `recall.jsonl` is moved into them on the next look).
pub fn file() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("friday/recall.jsonl")
}

fn segments_dir(path: &Path) -> PathBuf {
    path.with_file_name("recall")
}

/// The hourly files, oldest first, with their hour.
fn segments(path: &Path) -> Vec<(i64, PathBuf)> {
    let mut out: Vec<(i64, PathBuf)> = std::fs::read_dir(segments_dir(path))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let hour = p.file_stem()?.to_str()?.parse::<i64>().ok()?;
            (p.extension()? == "jsonl").then_some((hour, p))
        })
        .collect();
    out.sort();
    out
}

fn parse(text: &str) -> impl Iterator<Item = Seen> + '_ {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Seen>(l).ok())
}

/// What is kept and not too old, oldest first.
pub fn load(path: &Path, now: i64) -> Vec<Seen> {
    let keep = keep_secs(path);
    let mut all: Vec<Seen> = parse(&std::fs::read_to_string(path).unwrap_or_default()).collect();
    for (hour, p) in segments(path) {
        if (hour + 1) * HOUR > now - keep {
            all.extend(parse(&std::fs::read_to_string(p).unwrap_or_default()));
        }
    }
    all.retain(|s| (0..=keep).contains(&(now - s.ts)));
    all.sort_by_key(|s| s.ts);
    all
}

/// Keeps a look, unless the same window's newest look (in the last two
/// hours) had the same text. Hours older than the keep, and the oldest past
/// [`MAX_BYTES`], are dropped either way.
pub fn add(path: &Path, seen: Seen) -> std::io::Result<bool> {
    migrate(path)?;
    let hour = seen.ts.div_euclid(HOUR);
    let recent: Vec<Seen> = segments(path)
        .into_iter()
        .filter(|(h, _)| *h == hour || *h == hour - 1)
        .flat_map(|(_, p)| {
            parse(&std::fs::read_to_string(p).unwrap_or_default()).collect::<Vec<_>>()
        })
        .collect();
    let same = recent
        .iter()
        .filter(|s| s.app == seen.app && s.title == seen.title && s.ts <= seen.ts)
        .max_by_key(|s| s.ts)
        .is_some_and(|s| s.text == seen.text);
    if !same {
        append(&segments_dir(path).join(format!("{hour}.jsonl")), &seen)?;
    }
    prune(path, seen.ts);
    Ok(!same)
}

fn append(file: &Path, seen: &Seen) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(file)?;
    // One write per look, so a reader never sees half a line.
    f.write_all(format!("{}\n", serde_json::to_string(seen).unwrap_or_default()).as_bytes())
}

/// Drops the hours past the keep, then the oldest while the rest is over
/// [`MAX_BYTES`] (never the newest hour).
fn prune(path: &Path, now: i64) {
    let keep = keep_secs(path);
    let mut kept = Vec::new();
    for (hour, p) in segments(path) {
        if (hour + 1) * HOUR <= now - keep {
            let _ = std::fs::remove_file(p);
        } else {
            let size = std::fs::metadata(&p).map(|m| m.len() as usize).unwrap_or(0);
            kept.push((p, size));
        }
    }
    let mut total: usize = kept.iter().map(|(_, n)| n).sum();
    for (p, n) in kept.iter().take(kept.len().saturating_sub(1)) {
        if total <= MAX_BYTES {
            break;
        }
        let _ = std::fs::remove_file(p);
        total -= n;
    }
}

/// An older FRIDAY's single file: its looks go into the hourly files, then
/// it goes.
fn migrate(path: &Path) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    for s in parse(&text) {
        append(
            &segments_dir(path).join(format!("{}.jsonl", s.ts.div_euclid(HOUR))),
            &s,
        )?;
    }
    std::fs::remove_file(path)
}

fn ago(secs: i64) -> String {
    match secs {
        ..=89 => "just now".to_string(),
        90..=3599 => format!("{} min ago", ((secs + 30) / 60).min(59)),
        3600..=86_399 => format!("{} h {} min ago", secs / 3600, (secs % 3600) / 60),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// The passages that best answer the question, newest look of each window
/// first, as `(app — title, when)` and the text.
pub fn search(path: &Path, now: i64, question: &str) -> Vec<Excerpt> {
    let mut all = load(path, now);
    all.reverse();
    let text: String = all
        .iter()
        .map(|s| {
            format!(
                "{TITLE_MARK}{} — {} ({})\n{}\n",
                s.app,
                s.title,
                ago(now - s.ts),
                s.text
            )
        })
        .collect();
    local_docs::passages(question, &text)
}

/// The windows whose text is never kept (as `friday_private_window` in the
/// CLI): matched, case aside, against the app and the title. The person's
/// own list (`~/.config/friday/private-windows`) adds lines; "!word" drops a
/// built-in one.
pub const PRIVATE: &[&str] = &[
    "keepassxc",
    "bitwarden",
    "1password",
    "enpass",
    "proton pass",
    "org.gnome.world.secrets",
    "seahorse",
    "private browsing",
    "incognito",
    "inprivate",
    "bank",
    "paypal",
];

/// The person's own list of private windows (`private-windows` in FRIDAY's
/// config folder; `%APPDATA%\\friday` on Windows), or nothing.
pub fn own_list() -> String {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    base.map(|b| b.join("friday").join("private-windows"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

/// Whether this window is private, given the person's own list.
pub fn private(app: &str, title: &str, own: &str) -> bool {
    let hay = format!("{app} {title}").to_lowercase();
    let dropped: Vec<String> = own
        .lines()
        .filter_map(|l| l.trim().strip_prefix('!'))
        .map(str::to_lowercase)
        .collect();
    // "never <name>" is private too (and stricter for the CLI); folder
    // lines (/…, ~/… or C:\…) are about commands, not windows.
    PRIVATE
        .iter()
        .map(|p| p.to_string())
        .chain(own.lines().map(|l| {
            let l = l.trim().to_lowercase();
            l.strip_prefix("never ")
                .map_or(l.clone(), |r| r.trim().to_string())
        }))
        .filter(|p| !p.is_empty() && !p.starts_with('#') && !p.starts_with('!'))
        .filter(|p| !p.starts_with('/') && !p.starts_with("~/") && !p.contains(":\\"))
        .filter(|p| !dropped.contains(p))
        .any(|p| has_words(&hay, &p))
}

/// Whether `hay` has `words` as whole words ("bank" is in "My Bank —", not
/// in "Embankment").
fn has_words(hay: &str, words: &str) -> bool {
    hay.match_indices(words).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + words.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        std::env::temp_dir()
            .join(format!("friday-recall-{}", uuid::Uuid::new_v4()))
            .join("recall.jsonl")
    }

    fn seen(ts: i64, app: &str, text: &str) -> Seen {
        Seen {
            ts,
            app: app.into(),
            title: "t".into(),
            text: text.into(),
        }
    }

    #[test]
    fn the_oldest_hours_go_past_the_size_cap() {
        let p = tmp();
        let big = "x".repeat(MAX_BYTES / 2 + 1);
        add(&p, seen(1_000, "a", &big)).unwrap();
        add(&p, seen(1_000 + HOUR, "b", &big)).unwrap();
        add(&p, seen(1_000 + 2 * HOUR, "c", "small")).unwrap();
        let kept: Vec<String> = load(&p, 1_001 + 2 * HOUR)
            .into_iter()
            .map(|s| s.app)
            .collect();
        assert_eq!(kept, ["b", "c"]);
    }

    #[test]
    fn looks_are_appended_to_hourly_files_never_rewritten() {
        let p = tmp();
        add(&p, seen(7_200, "kitty", "one")).unwrap();
        let first = segments(&p);
        assert_eq!(first.len(), 1);
        let before = std::fs::read_to_string(&first[0].1).unwrap();
        add(&p, seen(7_230, "kitty", "two")).unwrap();
        let after = std::fs::read_to_string(&first[0].1).unwrap();
        assert!(after.starts_with(&before), "the first look was rewritten");
        add(&p, seen(7_200 + HOUR, "kitty", "three")).unwrap();
        assert_eq!(segments(&p).len(), 2, "a new hour, a new file");
        assert!(!p.exists(), "no single file");
    }

    #[test]
    fn an_older_single_file_moves_into_the_hours() {
        let p = tmp();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            format!(
                "{}\n",
                serde_json::to_string(&seen(1_000, "kitty", "old look")).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(load(&p, 1_100).len(), 1, "read before it moves");
        add(&p, seen(1_100, "firefox", "a page")).unwrap();
        assert!(!p.exists());
        let texts: Vec<String> = load(&p, 1_200).into_iter().map(|s| s.text).collect();
        assert_eq!(texts, ["old look", "a page"]);
    }

    #[test]
    fn a_longer_keep_is_read_beside_the_file() {
        let p = tmp();
        add(&p, seen(1_000, "firefox", "a page")).unwrap();
        assert!(load(&p, 1_000 + KEEP_SECS + 1).is_empty());
        std::fs::write(p.with_file_name("recall-keep"), "24\n").unwrap();
        assert_eq!(keep_secs(&p), 24 * 3600);
        assert_eq!(load(&p, 1_000 + KEEP_SECS + 1).len(), 1);
        assert!(load(&p, 1_000 + 24 * 3600 + 1).is_empty());
        // Only the offered keeps count: anything else is three hours.
        std::fs::write(p.with_file_name("recall-keep"), "9999").unwrap();
        assert_eq!(keep_secs(&p), KEEP_SECS);
        std::fs::write(p.with_file_name("recall-keep"), "168").unwrap();
        assert_eq!(keep_secs(&p), 7 * 24 * 3600);
        assert_eq!(ago(3 * 86_400 + 5), "3 days ago");
    }

    /// Everything on disk, as text.
    fn on_disk(p: &Path) -> String {
        segments(p)
            .into_iter()
            .map(|(_, f)| std::fs::read_to_string(f).unwrap())
            .collect()
    }

    #[test]
    fn looks_are_kept_once_owner_only_and_for_three_hours() {
        let p = tmp();
        assert!(add(&p, seen(1_000, "kitty", "error: linker cc not found")).unwrap());
        assert!(!add(&p, seen(1_030, "kitty", "error: linker cc not found")).unwrap());
        assert!(add(&p, seen(1_060, "kitty", "now it builds")).unwrap());
        assert_eq!(load(&p, 1_100).len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |f: &Path| std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&segments(&p)[0].1), 0o600);
            assert_eq!(mode(&segments_dir(&p)), 0o700);
        }
        // Past the keep the first ones are gone, from disk too, even when
        // nothing new is kept.
        add(&p, seen(1_060 + KEEP_SECS + 2 * HOUR, "firefox", "a page")).unwrap();
        let left = on_disk(&p);
        assert!(!left.contains("linker"), "{left}");
        let p2 = tmp();
        add(&p2, seen(0, "kitty", "old text")).unwrap();
        add(&p2, seen(KEEP_SECS - 10, "firefox", "a page")).unwrap();
        assert!(!add(&p2, seen(KEEP_SECS + 5, "firefox", "a page")).unwrap());
        let texts: Vec<String> = load(&p2, KEEP_SECS + 5)
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert_eq!(texts, ["a page"], "an expired look isn't read");
        // Its file goes once its whole hour is past the keep.
        add(&p2, seen(KEEP_SECS + HOUR, "kitty", "new text")).unwrap();
        let left = on_disk(&p2);
        assert!(
            !left.contains("old text") && left.contains("a page"),
            "{left}"
        );
        // A look from the future (the clock went back) isn't kept on.
        assert!(load(&p, 0).is_empty());
    }

    #[test]
    fn a_question_finds_what_was_on_screen() {
        let p = tmp();
        add(
            &p,
            seen(
                0,
                "firefox",
                "Rust 1.91 release notes: let chains are stable now",
            ),
        )
        .unwrap();
        add(
            &p,
            seen(
                600,
                "kitty",
                "error[E0425]: cannot find value `cfg` in this scope",
            ),
        )
        .unwrap();
        let found = search(&p, 900, "what was that cannot find value error?");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].source, "kitty — t (5 min ago)");
        assert!(found[0].text.contains("E0425"));
        assert!(search(&p, 900, "what's the weather in Paris").is_empty());
    }

    #[test]
    fn private_windows_are_never_kept() {
        assert!(private("org.keepassxc.KeePassXC", "Passwords.kdbx", ""));
        assert!(private(
            "thunderbird",
            "Inbox - work",
            "never thunderbird\n"
        ));
        assert!(!private(
            "code",
            "notes",
            "~/work/clients\n/srv/private\nC:\\Users\\me\\code\n"
        ));
        assert!(private("firefox", "My Bank — Mozilla Firefox", ""));
        assert!(!private("firefox", "My Bank — Mozilla Firefox", "!bank"));
        assert!(private("org.gnome.TextEditor", "diary.txt", "Diary\n"));
        assert!(!private("firefox", "Victoria Embankment — Maps", ""));
        assert!(private("firefox", "Online banking | My bank", ""));
        assert!(private("chromium", "New Incognito Tab", ""));
        assert!(!private("kitty", "zsh", "# a comment\n"));
    }
}
