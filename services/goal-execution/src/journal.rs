// ── Persistent Memory Journal ────────────────────────────────────────
//
// S2 (memory write + recall): a durable append-only JSONL journal that makes
// the Executive's episodic memory survive process restarts. This is NOT a
// second memory — the journal is the *same* `CognitiveEpisode` store, just
// persisted to disk. Every episode the admission gate accepts is appended to
// the journal line-by-line; on startup the Executive replays the journal to
// seed its `EpisodeStore`, so FRIDAY knows tomorrow what mattered today.
use crate::memory::CognitiveEpisode;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ── Recall scoring ──────────────────────────────────────────────────

/// Tokens that carry meaning for memory retrieval. Stops are dropped, and
/// short tokens carry little signal, so both are filtered out.
const STOPWORDS: &[&str] = &[
    "about", "above", "after", "again", "against", "also", "because", "been", "before", "being",
    "below", "between", "both", "could", "did", "does", "doing", "down", "during", "each", "few",
    "for", "from", "further", "have", "having", "here", "how", "into", "just", "more", "most",
    "much", "must", "only", "other", "ought", "over", "same", "shall", "should", "some", "such",
    "than", "that", "their", "them", "then", "there", "these", "they", "this", "those", "through",
    "under", "until", "very", "was", "were", "what", "when", "where", "which", "while", "with",
    "would", "your", "told", "tell", "whats", "please",
];

/// Lowercase alphanumeric tokens that carry meaning, each in its plain form
/// ("crashes" and "crashed" are "crash"): words of at least 4 chars that are
/// not stopwords, and codes, which error questions turn on: letters with
/// digits ("e0308", "ts2322") or a three-digit number ("404"). Years and
/// small numbers aren't codes.
pub fn significant_tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| {
            let digits = t.chars().filter(char::is_ascii_digit).count();
            if digits == 0 {
                t.len() >= 4 && !STOPWORDS.contains(t)
            } else {
                (digits < t.len() && t.len() <= 16) || (digits == 3 && t.len() == 3)
            }
        })
        .map(plain)
        .collect()
}

/// Whether `t` is a code (has a digit).
fn is_code(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_digit())
}

/// A word without its common English endings, so its forms match: a plural
/// first ("settings", "caches", "libraries"), then "-ing" or "-ed".
fn plain(word: &str) -> String {
    if is_code(word) {
        return word.to_string();
    }
    let mut w = word.to_string();
    if w.len() >= 5 && w.ends_with("ies") {
        w = format!("{}y", &w[..w.len() - 3]);
    } else if w.len() >= 6
        && ["ses", "xes", "zes", "ches", "shes"]
            .iter()
            .any(|e| w.ends_with(e))
    {
        w.truncate(w.len() - 2);
    } else if w.len() >= 4 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") {
        w.truncate(w.len() - 1);
    }
    if w.len() >= 6 && w.ends_with("ing") {
        w.truncate(w.len() - 3);
    } else if w.len() >= 5 && w.ends_with("ed") {
        w.truncate(w.len() - 2);
    } else if w.len() >= 5 && w.ends_with('e') {
        // "cache" and "caches" alike ("caches" lost its "es" as "matches" does).
        w.truncate(w.len() - 1);
    }
    w
}

/// How many of the query's tokens `text` shares, each counted once.
pub fn shared_tokens(query: &str, text: &str) -> usize {
    let text_tokens = significant_tokens(text);
    significant_tokens(query)
        .iter()
        .filter(|tok| text_tokens.contains(tok))
        .count()
}

/// How much `text` has to do with `query`: the query's tokens it shares, and
/// twice more for each shared code (an error number names the problem).
pub fn recall_score(query: &str, text: &str) -> usize {
    let query_tokens = significant_tokens(query);
    let text_tokens = significant_tokens(text);
    query_tokens
        .iter()
        .filter(|tok| text_tokens.contains(tok))
        .map(|tok| if is_code(tok) { 3 } else { 1 })
        .sum()
}

// ── Journal ─────────────────────────────────────────────────────────

/// Append-only, durable, shareable memory journal.
///
/// - `open` loads every valid line into memory (and transparently skips
///   corrupt lines so a bad write never bricks memory).
/// - `append` writes one JSONL line and updates the in-memory cache.
/// - `recall` returns the top-`limit` entries by token overlap with a query.
///
/// The `Arc` makes the journal safe to share between the Executive (writer)
/// and any recall tool (reader) without a second store.
pub struct MemoryJournal {
    path: PathBuf,
    episodes: Mutex<Vec<CognitiveEpisode>>,
}

impl MemoryJournal {
    /// Open (or create) a journal at `path`. Missing parent directories are
    /// created. Returns an `Arc` so callers can share the same journal.
    pub fn open(path: impl Into<PathBuf>) -> Result<Arc<Self>, String> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let mut loaded = Vec::new();
        if path.exists() {
            let file =
                fs::File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
            for line in BufReader::new(file).lines() {
                let line = line.map_err(|e| format!("read {}: {e}", path.display()))?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<CognitiveEpisode>(&line) {
                    Ok(episode) => loaded.push(episode),
                    Err(_) => tracing::warn!(
                        path = %path.display(),
                        "skipping unparseable memory journal line"
                    ),
                }
            }
        }
        Ok(Arc::new(Self {
            path,
            episodes: Mutex::new(loaded),
        }))
    }

    /// Append an episode as one JSONL line, in one write, synced to disk. A
    /// line torn by a crash is ended first, so it can't swallow this one.
    pub fn append(&self, episode: &CognitiveEpisode) {
        let line = match serde_json::to_string(episode) {
            Ok(line) => line,
            Err(e) => {
                tracing::error!("journal serialization failed: {e}");
                return;
            }
        };
        self.episodes.lock().unwrap().push(episode.clone());
        let torn = fs::File::open(&self.path)
            .and_then(|mut f| {
                use std::io::{Read, Seek, SeekFrom};
                if f.seek(SeekFrom::End(0))? == 0 {
                    return Ok(false);
                }
                f.seek(SeekFrom::End(-1))?;
                let mut last = [0u8; 1];
                f.read_exact(&mut last)?;
                Ok(last[0] != b'\n')
            })
            .unwrap_or(false);
        let mut bytes = Vec::with_capacity(line.len() + 2);
        if torn {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        let wrote = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| {
                file.write_all(&bytes)?;
                file.sync_data()
            });
        if let Err(e) = wrote {
            tracing::error!("journal append to {} failed: {e}", self.path.display());
        }
    }

    /// Recall the top `limit` entries whose text overlaps `query`.
    pub fn recall(&self, query: &str, limit: usize) -> Vec<CognitiveEpisode> {
        let episodes = self.episodes.lock().unwrap();
        let mut scored: Vec<(usize, &CognitiveEpisode)> = episodes
            .iter()
            .map(|ep| (recall_score(query, &ep.recall_text()), ep))
            .filter(|(score, _)| *score > 0)
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.started_ms.cmp(&a.1.started_ms))
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(_, ep)| ep.clone())
            .collect()
    }

    /// Number of episodes currently journaled.
    pub fn len(&self) -> usize {
        self.episodes.lock().unwrap().len()
    }

    /// True when the journal holds no episodes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All episodes currently loaded (used to seed an `EpisodeStore`).
    pub fn episodes(&self) -> Vec<CognitiveEpisode> {
        self.episodes.lock().unwrap().clone()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_of_a_word_and_error_codes_match() {
        assert!(
            recall_score(
                "why does firefox keep crashing",
                "Firefox crashed on start: a bad extension"
            ) >= 2
        );
        assert!(recall_score("what does E0308 mean", "error[E0308]: mismatched types") >= 3);
        assert!(
            recall_score("npm ERR 404 on install", "npm install failed with 404")
                > recall_score("npm ERR 404 on install", "npm install failed"),
            "the code counts most"
        );
        assert_eq!(plain("libraries"), "library");
        assert_eq!(plain("settings"), plain("setting"));
        assert_eq!(plain("caches"), plain("cache"));
        assert_eq!(plain("strings"), plain("string"));
        assert_eq!(plain("status"), "status");
        assert_eq!(
            significant_tokens("in 2026 at 10 got a 404"),
            vec!["404".to_string()],
            "years and small numbers aren't codes"
        );
        assert_eq!(plain("process"), "process", "a double s stays");
        assert_eq!(plain("bus"), "bus", "short words stay");
    }
    use crate::goal::Goal;
    use crate::memory::{CognitiveEpisode, EpisodeOutcome};
    use crate::verification::ToolResult;

    fn temp_journal_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "friday-memory-journal-{tag}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn fact_episode(fact: &str) -> CognitiveEpisode {
        CognitiveEpisode::begin(Goal::new(fact))
            .with_execution_result(ToolResult::success("acknowledged"))
            .with_tag("fact".to_string())
            .finish(EpisodeOutcome::Success)
    }

    #[test]
    fn significant_tokens_drops_stopwords_and_short_words() {
        let tokens = significant_tokens("What was that project I told you about?");
        assert_eq!(tokens, vec!["project".to_string()]);
        let tokens = significant_tokens("My main project is FRIDAY");
        assert_eq!(
            tokens,
            vec![
                "main".to_string(),
                "project".to_string(),
                "friday".to_string()
            ]
        );
    }

    #[test]
    fn recall_score_counts_only_shared_tokens() {
        assert_eq!(
            recall_score("what was that project", "my main project is friday"),
            1
        );
        assert_eq!(
            recall_score("show the weather", "my main project is friday"),
            0
        );
        assert_eq!(recall_score("project", "project"), 1);
    }

    #[test]
    fn journal_round_trips_episodes_and_recalls_them() {
        let path = temp_journal_path("roundtrip");
        let journal = MemoryJournal::open(&path).expect("open journal");
        assert!(journal.is_empty());

        journal.append(&fact_episode("My main project is FRIDAY"));
        journal.append(&fact_episode("The coffee machine lives in the kitchen"));
        assert_eq!(journal.len(), 2);

        // Reopen the same file as a brand-new journal: the durability proof.
        let reopened = MemoryJournal::open(&path).expect("reopen journal");
        assert_eq!(reopened.len(), 2);

        let hits = reopened.recall("What was that project I told you about?", 3);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].goal.description.contains("project"));

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn journal_skips_corrupt_lines() {
        let path = temp_journal_path("corrupt");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&path).expect("create");
            writeln!(file, "this is not json").expect("write bad line");
            let good =
                serde_json::to_string(&fact_episode("A valid fact is stored")).expect("serialize");
            writeln!(file, "{good}").expect("write good line");
        }
        let journal = MemoryJournal::open(&path).expect("open journal");
        assert_eq!(journal.len(), 1);
        assert_eq!(
            journal.episodes()[0].goal.description,
            "A valid fact is stored"
        );
        let _ = std::fs::remove_dir_all(&path);
    }

    /// Killed halfway through a line (kill -9, power cut): the torn line is
    /// skipped and the next fact starts on a line of its own, so nothing
    /// saved before or after the crash is lost.
    #[test]
    fn a_torn_last_line_never_swallows_the_next_fact() {
        let path = temp_journal_path("torn");
        let journal = MemoryJournal::open(&path).expect("open journal");
        journal.append(&fact_episode("My main project is FRIDAY"));
        let half = serde_json::to_string(&fact_episode("cut off")).unwrap();
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&half.as_bytes()[..half.len() / 2]).unwrap();
        }
        let after = MemoryJournal::open(&path).expect("reopen after the crash");
        assert_eq!(after.len(), 1);
        after.append(&fact_episode("The coffee machine lives in the kitchen"));
        let facts: Vec<String> = MemoryJournal::open(&path)
            .expect("reopen")
            .episodes()
            .into_iter()
            .map(|e| e.goal.description)
            .collect();
        assert_eq!(
            facts,
            vec![
                "My main project is FRIDAY".to_string(),
                "The coffee machine lives in the kitchen".to_string()
            ]
        );
        let _ = std::fs::remove_file(&path);
    }
}
