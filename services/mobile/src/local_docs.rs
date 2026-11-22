//! `friday-ctl docs "<question>"`: short excerpts from documentation that is
//! already on this computer, for the model to answer from (and name): the Arch
//! wiki (`arch-wiki-lite`, plain text), tldr pages (`tealdeer`'s cache), man
//! pages, and FRIDAY's own guide for questions about FRIDAY. Nothing is sent
//! anywhere; with nothing installed, or nothing that matches well, it prints
//! nothing.
//!
//! The wiki ships as one gzip file of `NNNNNNNN:line` lines plus an index of
//! `lang/Title NNNNNNNN`. The English articles are unpacked once into a cache
//! file (`friday-ctl docs --prepare`); a search is a plain scan of it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const WIKI_DIR: &str = "/usr/share/doc/arch-wiki/text";
/// Marks an article's first line in the cache: `\x01Title`.
pub const TITLE_MARK: char = '\u{1}';
const PASSAGE_LINES: usize = 14;
const PASSAGE_CHARS: usize = 1500;
const PAGE_CHARS: usize = 1200;

/// Where the documentation and the cache are.
pub struct Sources {
    pub wiki_gz: PathBuf,
    pub wiki_index: PathBuf,
    pub cache: PathBuf,
    /// tldr pages (`.../pages.en` or `.../pages`), if fetched.
    pub tldr: Option<PathBuf>,
    pub man: bool,
    /// The text of the folders the person chose to search (`friday folders`),
    /// one article per file, titled with its path.
    pub folders: PathBuf,
    /// FRIDAY's guide (`USING.md`) as installed with this version.
    pub guide: PathBuf,
}

impl Sources {
    pub fn system() -> Self {
        let cache_home = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let tealdeer = std::env::var_os("TEALDEER_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| cache_home.join("tealdeer"));
        let tldr = [
            "tldr-pages/pages.en",
            "tldr-pages/pages",
            "tldr-master/pages",
        ]
        .iter()
        .map(|p| tealdeer.join(p))
        .find(|p| p.is_dir());
        Sources {
            wiki_gz: Path::new(WIKI_DIR).join("arch-wiki.txt.gz"),
            wiki_index: Path::new(WIKI_DIR).join("index"),
            cache: cache_home.join("friday/arch-wiki-en.txt"),
            tldr,
            man: true,
            folders: cache_home.join("friday/folders.txt"),
            // Beside friday-ctl.exe on Windows; the package's doc folder on Linux.
            guide: if cfg!(windows) {
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(|d| d.join("USING.md")))
                    .unwrap_or_default()
            } else {
                PathBuf::from("/usr/share/doc/friday/USING.md")
            },
        }
    }

    /// Whether the cache is missing or older than the wiki it came from.
    pub fn needs_prepare(&self) -> bool {
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        match (modified(&self.wiki_gz), modified(&self.cache)) {
            (Some(wiki), Some(cache)) => cache < wiki,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// One excerpt: where it's from ("Arch wiki: Bluetooth") and the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Excerpt {
    pub source: String,
    pub text: String,
}

/// Unpacks the English wiki articles into the cache (written whole, then
/// renamed, so a reader never sees half of it).
pub fn prepare(s: &Sources) -> std::io::Result<()> {
    let mut titles: HashMap<String, String> = HashMap::new();
    for line in std::fs::read_to_string(&s.wiki_index)?.lines() {
        if let Some((path, num)) = line.rsplit_once(' ') {
            if let Some(title) = path.strip_prefix("en/") {
                titles.insert(num.to_string(), title.replace('_', " "));
            }
        }
    }
    let gz = std::fs::File::open(&s.wiki_gz)?;
    let mut reader = BufReader::new(flate2::read::GzDecoder::new(gz));
    if let Some(dir) = s.cache.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = s.cache.with_extension(format!("tmp{}", std::process::id()));
    let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let mut current = String::new();
    // Read as bytes: one line that isn't valid UTF-8 mustn't stop the rest.
    let mut buf = Vec::new();
    while reader.read_until(b'\n', &mut buf)? > 0 {
        let line = String::from_utf8_lossy(&buf).into_owned();
        buf.clear();
        let Some((num, text)) = line.trim_end_matches(['\n', '\r']).split_once(':') else {
            continue;
        };
        let Some(title) = titles.get(num) else {
            continue;
        };
        if num != current {
            writeln!(out, "{TITLE_MARK}{title}")?;
            current = num.to_string();
        }
        let text = text.replace("@@b", "").replace("@@w", "");
        let text = text.trim();
        if !text.is_empty() {
            writeln!(out, "{text}")?;
        }
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&tmp, &s.cache)
}

const STOP: &[&str] = &[
    "the", "and", "for", "with", "how", "what", "why", "when", "where", "which", "does", "can",
    "this", "that", "you", "your", "are", "was", "not", "but", "get", "use", "using", "into",
    "from", "have", "has", "there", "should", "would", "could", "about", "make", "need", "want",
    "way", "any", "all", "some", "out", "its", "it's", "isn't", "don't", "doesn't", "my", "me",
    "is", "do", "to", "in", "on", "of", "a", "an", "or", "it", "i", "be", "set", "work", "working",
    "linux", "arch", "computer",
];

/// Commands that are also everyday English: a tldr or man page is added for
/// them only when the question writes them as code (`like this`).
const COMMON_WORDS: &[&str] = &[
    "find", "time", "date", "free", "watch", "kill", "less", "more", "top", "sort", "head", "tail",
    "cut", "join", "split", "file", "touch", "test", "true", "false", "yes", "who", "man", "make",
    "install", "ping", "which", "type", "read", "wait", "sleep", "history", "export", "alias",
    "say", "look", "write", "last", "link", "open", "print", "paste", "stat", "wall", "users",
    "groups", "clear", "reset", "column", "fold", "env", "help", "start",
];

fn terms(question: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in question
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '\''))
    {
        let w = w.trim_matches(|c| c == '-' || c == '\'');
        if w.chars().count() >= 3 && !STOP.contains(&w) && !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// Words the question writes as code, or command-like words that aren't
/// everyday English.
fn command_words(question: &str) -> Vec<String> {
    let quoted: Vec<String> = question
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|q| q.split_whitespace().next())
        .map(str::to_lowercase)
        .collect();
    let mut out = quoted.clone();
    for t in terms(question) {
        if !COMMON_WORDS.contains(&t.as_str()) && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

struct Article<'a> {
    title: &'a str,
    lines: Vec<&'a str>,
}

fn articles(text: &str) -> Vec<Article<'_>> {
    let mut out: Vec<Article> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix(TITLE_MARK) {
            out.push(Article {
                title,
                lines: Vec::new(),
            });
        } else if let Some(a) = out.last_mut() {
            a.lines.push(line);
        }
    }
    out
}

/// How often a term appears, in the ways the wiki spells it.
fn count(hay: &str, term: &str) -> usize {
    let spellings: &[&str] = match term {
        "wifi" => &["wifi", "wi-fi", "wireless"],
        _ => &[],
    };
    if spellings.is_empty() {
        hay.matches(term).count()
    } else {
        spellings.iter().map(|s| hay.matches(s).count()).sum()
    }
}

/// The best articles in `cache_text` (articles start with a `TITLE_MARK`
/// line) for the question: one, or two when the second scores close, each
/// with its best passage, where the match is strong (the passage holds most
/// of the question's words). The source is the article's title.
pub fn passages(question: &str, cache_text: &str) -> Vec<Excerpt> {
    let terms = terms(question);
    let arts = articles(cache_text);
    if terms.is_empty() || arts.is_empty() {
        return Vec::new();
    }
    // Per article: how often each term appears, and its length.
    let lowered: Vec<String> = arts
        .iter()
        .map(|a| a.lines.join("\n").to_lowercase())
        .collect();
    let tfs: Vec<Vec<usize>> = lowered
        .iter()
        .map(|body| terms.iter().map(|t| count(body, t)).collect())
        .collect();
    let n = arts.len() as f64;
    let avg = lowered.iter().map(|b| b.len()).sum::<usize>() as f64 / n;
    let idf: Vec<f64> = (0..terms.len())
        .map(|i| {
            let df = tfs.iter().filter(|tf| tf[i] > 0).count() as f64;
            ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
        })
        .collect();
    // BM25 over the body, plus a bonus for the share of the title the
    // question's words make up ("Pacman" over "Pacman/Package signing").
    // Category pages are lists; a laptop model's page (a title with digits)
    // counts half unless the question names it.
    let score = |k: usize| -> f64 {
        let title = arts[k].title.to_lowercase();
        if title.starts_with("category:") {
            return 0.0;
        }
        let title_chars = title.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64;
        let len = lowered[k].len() as f64;
        let mut in_title = 0.0;
        let mut sum: f64 = 0.0;
        for (i, t) in terms.iter().enumerate() {
            let tf = tfs[k][i] as f64;
            sum += idf[i] * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * len / avg));
            if count(&title, t) > 0 {
                in_title += 1.0;
                sum += 3.0 * idf[i] * (t.chars().count() as f64 / title_chars).min(1.0);
            }
        }
        if in_title == 0.0 && title.chars().any(|c| c.is_ascii_digit()) {
            sum /= 2.0;
        }
        sum
    };
    let mut ranked: Vec<(f64, usize)> = (0..arts.len()).map(|k| (score(k), k)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let first = ranked[0].0;
    ranked
        .iter()
        .take(2)
        .filter(|(sc, _)| *sc > 0.0 && *sc >= 0.8 * first)
        .filter_map(|&(_, k)| passage(&arts[k], &terms, &idf))
        .collect()
}

/// An article's best passage, if it holds enough of the question's words.
fn passage(art: &Article, terms: &[String], idf: &[f64]) -> Option<Excerpt> {
    // The window of lines that holds the most of the question's words, the
    // rarer ones counting more.
    let lines: Vec<String> = art.lines.iter().map(|l| l.to_lowercase()).collect();
    let mut top: Option<(f64, usize, usize, usize)> = None; // (weight, hits, distinct, start)
    for start in 0..lines.len() {
        let window = lines[start..(start + PASSAGE_LINES).min(lines.len())].join("\n");
        let found: Vec<usize> = (0..terms.len())
            .filter(|&i| count(&window, &terms[i]) > 0)
            .collect();
        let weight: f64 = found.iter().map(|&i| idf[i]).sum();
        let hits: usize = terms.iter().map(|t| count(&window, t)).sum();
        if top.is_none_or(|(w, h, _, _)| weight > w || (weight == w && hits > h)) {
            top = Some((weight, hits, found.len(), start));
        }
    }
    let (_, _, distinct, start) = top?;
    let title = art.title.to_lowercase();
    let title_terms = terms.iter().filter(|t| count(&title, t) > 0).count();
    let needed = (terms.len() * 3).div_ceil(5).max(1);
    if distinct + title_terms.min(1) < needed || distinct == 0 {
        return None;
    }
    let text = clip(
        &art.lines[start..(start + PASSAGE_LINES).min(art.lines.len())].join("\n"),
        PASSAGE_CHARS,
    );
    Some(Excerpt {
        source: art.title.to_string(),
        text,
    })
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// A command name that is safe as a file name and as an argument.
fn plain_command(cmd: &str) -> bool {
    !cmd.starts_with(['-', '.'])
        && cmd
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
}

/// The tldr page for a command, if fetched.
fn tldr(dir: &Path, cmd: &str) -> Option<Excerpt> {
    if !plain_command(cmd) {
        return None;
    }
    ["common", "linux"].iter().find_map(|section| {
        let text = std::fs::read_to_string(dir.join(section).join(format!("{cmd}.md"))).ok()?;
        Some(Excerpt {
            source: format!("tldr: {cmd}"),
            text: clip(text.trim(), PAGE_CHARS),
        })
    })
}

/// The start of a command's man page (its name, synopsis and description).
fn man(cmd: &str) -> Option<Excerpt> {
    if !plain_command(cmd) {
        return None;
    }
    let out = Command::new("man")
        .args(["-P", "cat", "--", cmd])
        .env("MANWIDTH", "80")
        .env("MAN_KEEP_FORMATTING", "")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let text: String = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(25)
        .collect::<Vec<_>>()
        .join("\n");
    Some(Excerpt {
        source: format!("man {cmd}"),
        text: clip(&text, PAGE_CHARS),
    })
}

/// Whether a question is about FRIDAY itself: it names FRIDAY, a hotkey or a
/// shortcut.
pub fn about_friday(question: &str) -> bool {
    question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| {
            [
                "friday",
                "friday's",
                "hotkey",
                "hotkeys",
                "shortcut",
                "shortcuts",
            ]
            .contains(&w)
        })
}

/// The guide as articles, one per `##` section, titled with its heading.
fn guide_articles(guide: &str) -> String {
    let mut out = String::new();
    for line in guide.lines() {
        match line.strip_prefix("## ") {
            Some(h) => out.push_str(&format!("{TITLE_MARK}{}\n", h.trim())),
            // The page title and the lead before the first section.
            None if out.is_empty() => out.push_str(&format!("{TITLE_MARK}Using FRIDAY\n{line}\n")),
            None => out.push_str(&format!("{line}\n")),
        }
    }
    out
}

/// Excerpts for the question: FRIDAY's guide when it's about FRIDAY, a
/// command page or two, then the wiki passage.
pub fn find(question: &str, s: &Sources) -> Vec<Excerpt> {
    let mut out = Vec::new();
    if about_friday(question) && crate::features::on("self-docs") {
        if let Ok(guide) = std::fs::read_to_string(&s.guide) {
            // "friday" is in every section: it says nothing about which one.
            let asked = question
                .to_lowercase()
                .replace("friday's", "")
                .replace("friday", "");
            out.extend(
                passages(&asked, &guide_articles(&guide))
                    .into_iter()
                    .take(2)
                    .map(|e| Excerpt {
                        source: format!("FRIDAY guide: {}", e.source),
                        text: e.text,
                    }),
            );
        }
    }
    for cmd in command_words(question) {
        if out.len() == 2 {
            break;
        }
        if let Some(e) = s.tldr.as_deref().and_then(|d| tldr(d, &cmd)) {
            out.push(e);
        } else if s.man && question.to_lowercase().contains(&format!("`{cmd}")) {
            // A man page only for a command written as code: `man` itself
            // would match far too many ordinary words.
            out.extend(man(&cmd));
        }
    }
    if let Ok(text) = std::fs::read_to_string(&s.cache) {
        out.extend(
            passages(question, &text)
                .into_iter()
                .take(3 - out.len())
                .map(|e| Excerpt {
                    source: format!("Arch wiki: {}", e.source),
                    text: e.text,
                }),
        );
    }
    // The person's own folders: two passages at most, named by file.
    if let Ok(text) = std::fs::read_to_string(&s.folders) {
        out.extend(
            passages(question, &text)
                .into_iter()
                .take(2)
                .map(|e| Excerpt {
                    source: format!("File: {}", e.source),
                    text: e.text,
                }),
        );
    }
    out
}

/// Kinds of file read from a chosen folder (PDFs through `pdftotext`).
const FOLDER_TEXT: &[&str] = &["md", "txt", "org", "rst", "adoc", "tex"];
/// The most text kept from one file, and from all of them, in bytes.
const FILE_BYTES: usize = 1_000_000;
const FOLDERS_BYTES: usize = 50_000_000;

/// Writes the text of the notes and PDFs under `folders` to `cache`, one
/// article per file titled with its path; folders under `private` (and
/// hidden ones, `node_modules`, `target`) are skipped. Returns how many files
/// went in.
pub fn prepare_folders(
    folders: &[PathBuf],
    private: &[PathBuf],
    cache: &Path,
) -> std::io::Result<usize> {
    // Symlinks are never followed: they could lead into a private or hidden
    // place, or round in a loop.
    fn walk(dir: &Path, depth: usize, private: &[PathBuf], files: &mut Vec<PathBuf>) {
        if depth > 8 || private.iter().any(|p| dir.starts_with(p)) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries
            .flatten()
            .filter_map(|e| Some((e.path(), e.file_type().ok()?)))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, kind) in entries {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || kind.is_symlink()
            {
                continue;
            }
            if kind.is_dir() {
                walk(&path, depth + 1, private, files);
            } else if kind.is_file() && !private.iter().any(|p| path.starts_with(p)) {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for f in folders {
        walk(f, 0, private, &mut files);
    }
    if let Some(dir) = cache.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = cache.with_extension("tmp");
    // A new file every time, so it is created owner-only.
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut out = std::io::BufWriter::new(opts.open(&tmp)?);
    let (mut total, mut count) = (0usize, 0usize);
    for path in files {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        let text = if FOLDER_TEXT.contains(&ext.as_str()) {
            // Only the first megabyte is read: a huge file can't fill memory.
            let mut buf = Vec::new();
            std::fs::File::open(&path)
                .and_then(|f| {
                    std::io::Read::read_to_end(
                        &mut std::io::Read::take(f, FILE_BYTES as u64),
                        &mut buf,
                    )
                })
                .ok()
                .map(|_| String::from_utf8_lossy(&buf).into_owned())
        } else if ext == "pdf" {
            Command::new("pdftotext")
                .arg(&path)
                .arg("-")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        } else {
            None
        };
        let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let text = clip(&text.replace(TITLE_MARK, " "), FILE_BYTES);
        if total + text.len() > FOLDERS_BYTES {
            break;
        }
        total += text.len();
        count += 1;
        writeln!(out, "{TITLE_MARK}{}", path.display())?;
        writeln!(out, "{text}")?;
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&tmp, cache)?;
    Ok(count)
}

/// The excerpts as one block of text for a prompt ("" when there are none).
pub fn render(excerpts: &[Excerpt]) -> String {
    excerpts
        .iter()
        .map(|e| format!("[{}]\n{}\n", e.source, e.text))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn chosen_folders_are_searched_and_cited_by_file() {
        let (dir, s) = fixture();
        let notes = dir.join("notes");
        std::fs::create_dir_all(notes.join("private")).unwrap();
        std::fs::create_dir_all(notes.join(".git")).unwrap();
        std::fs::write(
            notes.join("router.md"),
            "# Router\n\nThe router admin password rotation happens every quarter on the first Monday.\n",
        )
        .unwrap();
        std::fs::write(
            notes.join("private/salary.md"),
            "router salary rotation quarter Monday",
        )
        .unwrap();
        std::fs::write(notes.join(".git/config"), "router rotation quarter").unwrap();
        std::fs::write(notes.join("photo.jpg"), [0xff, 0xd8, 0xff]).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(notes.join("private"), notes.join("linked")).unwrap();
            std::os::unix::fs::symlink(&notes, notes.join("loop")).unwrap();
        }
        let n = prepare_folders(
            std::slice::from_ref(&notes),
            &[notes.join("private")],
            &s.folders,
        )
        .unwrap();
        assert_eq!(n, 1);
        let found = find("when does the router password rotation happen", &s);
        let file = found
            .iter()
            .find(|e| e.source.starts_with("File: "))
            .expect("a file passage");
        assert!(
            Path::new(file.source.trim_start_matches("File: "))
                .ends_with(Path::new("notes").join("router.md")),
            "{}",
            file.source
        );
        assert!(file.text.contains("first Monday"), "{}", file.text);
        let all = std::fs::read_to_string(&s.folders).unwrap();
        assert!(!all.contains("salary"), "a private folder was indexed");
    }

    fn sources(dir: &Path) -> Sources {
        Sources {
            wiki_gz: dir.join("arch-wiki.txt.gz"),
            wiki_index: dir.join("index"),
            cache: dir.join("cache/arch-wiki-en.txt"),
            tldr: Some(dir.join("tldr")),
            man: false,
            folders: dir.join("cache/folders.txt"),
            guide: dir.join("USING.md"),
        }
    }

    #[test]
    fn questions_about_friday_are_answered_from_its_guide() {
        let dir = std::env::temp_dir().join(format!("friday-guide-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = sources(&dir);
        s.tldr = None;
        s.guide = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/USING.md");
        let found = find("how do I ask FRIDAY about the text I selected?", &s);
        assert!(
            found
                .first()
                .is_some_and(|e| e.source == "FRIDAY guide: The three keys"),
            "{found:?}"
        );
        let keys = find("which hotkey asks about my screen", &s);
        assert!(
            keys.iter().any(|e| e.source.starts_with("FRIDAY guide:")),
            "{keys:?}"
        );
        // A question that isn't about FRIDAY never gets the guide.
        let other = find("which key asks about my screen in gnome", &s);
        assert!(
            other.iter().all(|e| !e.source.starts_with("FRIDAY guide")),
            "{other:?}"
        );
        // No guide installed: nothing, and no error.
        s.guide = dir.join("missing.md");
        assert!(find("how do I ask FRIDAY about a selection", &s).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fixture() -> (PathBuf, Sources) {
        let dir = std::env::temp_dir().join(format!("friday-docs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("tldr/common")).unwrap();
        std::fs::write(
            dir.join("index"),
            "de/Bluetooth 00000001\nen/Bluetooth_headset 00000002\nen/Pacman 00000003\nen/Main_page 00000004\n",
        )
        .unwrap();
        let wiki = "\
00000001:Bluetooth auf Deutsch: Kopfhörer verbinden
00000002:@@bJump to content@@w
00000002:
00000002:Bluetooth headset
00000002:Headsets connect over Bluetooth.
00000002:To pair the headset, run bluetoothctl and type: power on, scan on, pair MAC, connect MAC.
00000002:If the headset connects but there is no sound, select the A2DP profile in pavucontrol.
00000003:Pacman is the package manager.
00000003:To upgrade the system, run pacman -Syu. Never do partial upgrades.
00000004:Welcome to the wiki. Bluetooth, pacman, and much more.
";
        let mut gz = flate2::write::GzEncoder::new(
            std::fs::File::create(dir.join("arch-wiki.txt.gz")).unwrap(),
            flate2::Compression::fast(),
        );
        gz.write_all(wiki.as_bytes()).unwrap();
        gz.finish().unwrap();
        std::fs::write(
            dir.join("tldr/common/rsync.md"),
            "# rsync\n\n> Transfer files.\n\n- Copy a folder:\n\n`rsync -a src/ dest/`\n",
        )
        .unwrap();
        std::fs::write(dir.join("tldr/common/find.md"), "# find\n\n> Find files.\n").unwrap();
        let s = sources(&dir);
        (dir, s)
    }

    #[test]
    fn prepare_keeps_english_articles_without_markup() {
        let (_dir, s) = fixture();
        assert!(s.needs_prepare());
        prepare(&s).unwrap();
        assert!(!s.needs_prepare());
        let text = std::fs::read_to_string(&s.cache).unwrap();
        assert!(text.contains("\u{1}Bluetooth headset\n"), "{text}");
        assert!(text.contains("Jump to content"), "{text}");
        assert!(!text.contains("@@"), "{text}");
        assert!(!text.contains("Deutsch"), "{text}");
    }

    #[test]
    fn a_setup_question_gets_the_matching_passage_and_its_title() {
        let (_dir, s) = fixture();
        prepare(&s).unwrap();
        let found = find("how do I pair my bluetooth headset?", &s);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].source, "Arch wiki: Bluetooth headset");
        assert!(found[0].text.contains("bluetoothctl"), "{found:?}");
        let found = find("how to upgrade with pacman", &s);
        assert_eq!(found[0].source, "Arch wiki: Pacman");
    }

    #[test]
    fn an_unrelated_question_gets_nothing() {
        let (_dir, s) = fixture();
        prepare(&s).unwrap();
        assert!(find("who won the football match yesterday", &s).is_empty());
        assert!(find("why", &s).is_empty());
    }

    #[test]
    fn a_named_command_brings_its_tldr_page_but_not_an_everyday_word() {
        let (_dir, s) = fixture();
        let found = find("copy a folder with rsync", &s);
        assert_eq!(found[0].source, "tldr: rsync");
        assert!(found[0].text.contains("rsync -a"));
        assert!(find("help me find my keys", &s).is_empty());
        assert_eq!(find("how does `find` work", &s)[0].source, "tldr: find");
        assert!(find("what does `-k` do", &s).is_empty());
        assert!(!plain_command("../etc/passwd") && !plain_command("-k"));
    }

    #[test]
    fn with_nothing_installed_there_is_nothing() {
        let dir = std::env::temp_dir().join(format!("friday-docs-{}", uuid::Uuid::new_v4()));
        let s = sources(&dir);
        assert!(!s.needs_prepare());
        assert!(find("how do I pair my bluetooth headset", &s).is_empty());
        assert_eq!(render(&[]), "");
    }
}
