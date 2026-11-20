//! `friday-ctl docs "<question>"`: short excerpts from documentation that is
//! already on this computer, for the model to answer from (and name): the Arch
//! wiki (`arch-wiki-lite`, plain text), tldr pages (`tealdeer`'s cache) and man
//! pages. Nothing is sent anywhere; with nothing installed, or nothing that
//! matches well, it prints nothing.
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

/// Excerpts for the question: a command page or two, then the wiki passage.
pub fn find(question: &str, s: &Sources) -> Vec<Excerpt> {
    let mut out = Vec::new();
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
    out
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

    fn sources(dir: &Path) -> Sources {
        Sources {
            wiki_gz: dir.join("arch-wiki.txt.gz"),
            wiki_index: dir.join("index"),
            cache: dir.join("cache/arch-wiki-en.txt"),
            tldr: Some(dir.join("tldr")),
            man: false,
        }
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
