//! Screen question benchmark: ranks candidate models on 8 fixed screen questions.
//!
//! Evaluates candidate models on 8 short window texts typical of screen asks
//! (error dialog, compiler error, settings, email, spreadsheet, calendar,
//! git status, web form) to pick the best model for screen questions.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A fixed screen question test case.
#[derive(Debug, Clone)]
pub struct Case {
    pub kind: &'static str,
    pub window_text: &'static str,
    pub question: &'static str,
    pub expected_keywords: &'static [&'static str],
}

impl Case {
    /// The formatted prompt presented to the model.
    pub fn prompt(&self) -> String {
        format!(
            "Window text:\n{}\n\nQuestion: {}\nAnswer concisely based only on the window text.",
            self.window_text, self.question
        )
    }
}

/// The 8 fixed screen cases (made-up names, no personal data).
pub const CASES: [Case; 8] = [
    Case {
        kind: "error dialog",
        window_text: "Connection Failed\nCould not connect to database at db.internal.local:5432.\nError: connection timed out after 30000ms.\n[Retry] [Cancel]",
        question: "What is the database port that failed to connect?",
        expected_keywords: &["5432"],
    },
    Case {
        kind: "terminal compiler error",
        window_text: "$ cargo check\nerror[E0308]: mismatched types\n  --> src/main.rs:42:5\n   |\n42 |     let x: u32 = \"hello\";\n   |            ---   ^^^^^^^ expected `u32`, found `&str`",
        question: "What compiler error code was reported?",
        expected_keywords: &["e0308"],
    },
    Case {
        kind: "settings page",
        window_text: "Settings > Network > Wi-Fi\nStatus: Connected to Northwind-Guest\nIPv4 Address: 192.168.1.145\nSecurity: WPA3-Personal",
        question: "What is the connected Wi-Fi network name?",
        expected_keywords: &["northwind-guest"],
    },
    Case {
        kind: "email",
        window_text: "Mail - Inbox\nFrom: alex.miller@example-corp.fake\nSubject: Launch date update\nHi team, our product release date has moved to October 24th.\nThanks,\nAlex",
        question: "What is the new release date mentioned in the email?",
        expected_keywords: &["october 24th"],
    },
    Case {
        kind: "spreadsheet row",
        window_text: "Spreadsheet - Q3 Inventory\nRow 14: SKU-8821 | Widget Pro | Qty: 450 | Unit Price: $12.50 | Total: $5625.00",
        question: "What is the quantity in stock for Widget Pro?",
        expected_keywords: &["450"],
    },
    Case {
        kind: "calendar",
        window_text: "Calendar - Tuesday, Nov 18\n10:00 AM - 11:00 AM: Sprint Review with Maya and Liam\nLocation: Conference Room B",
        question: "In which room is the sprint review meeting?",
        expected_keywords: &["room b"],
    },
    Case {
        kind: "git status",
        window_text: "$ git status\nOn branch feature/auth-token\nChanges not staged for commit:\n  modified:   src/auth.rs\nUntracked files:\n  tests/auth_test.rs",
        question: "Which modified file is not staged for commit?",
        expected_keywords: &["src/auth.rs"],
    },
    Case {
        kind: "web form",
        window_text: "Browser - Shipping Checkout\nCustomer: Jordan Lee\nShipping Method: Express Overnight ($15.00)\nPromo Code: SAVE20 applied (-$5.00)\nTotal: $35.00",
        question: "What promo code was applied to the order?",
        expected_keywords: &["save20"],
    },
];

/// Benchmark result for one model across all 8 cases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub model: String,
    pub correct: usize,
    pub median_ms: u64,
    pub failed: usize,
    /// It said "too many requests" (a free tier's limit), so it wasn't
    /// asked the rest: try again later.
    pub rate_limited: bool,
}

impl Row {
    /// True when the model errored on every case.
    pub fn unavailable(&self) -> bool {
        self.failed == CASES.len()
    }
}

/// Trait for asking a model a single screen question prompt.
#[async_trait]
pub trait Ask: Send + Sync {
    async fn ask(&self, model: &str, prompt: &str) -> Result<(String, u64), String>;
}

#[async_trait]
impl<F, Fut> Ask for F
where
    F: Fn(&str, &str) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(String, u64), String>> + Send,
{
    async fn ask(&self, model: &str, prompt: &str) -> Result<(String, u64), String> {
        (self)(model, prompt).await
    }
}

/// Run all 8 cases against each candidate model, ranking them by correct desc then median_ms asc.
/// Models that error on every case are placed last as unavailable.
pub async fn rank<S, A>(models: &[S], asker: A) -> Vec<Row>
where
    S: AsRef<str>,
    A: Ask,
{
    let mut rows = Vec::with_capacity(models.len());

    for model_ref in models {
        let model = model_ref.as_ref();
        let mut correct = 0;
        let mut failed = 0;
        let mut latencies = Vec::with_capacity(CASES.len());
        let mut rate_limited = false;

        for (i, case) in CASES.iter().enumerate() {
            if rate_limited {
                failed += 1;
                continue;
            }
            // Paced, so a free tier's rate limit doesn't count as wrong answers.
            if i > 0 && !cfg!(test) {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
            let prompt = case.prompt();
            match asker.ask(model, &prompt).await {
                Ok((text, ms)) => {
                    latencies.push(ms);
                    let lower = text.to_lowercase();
                    if case
                        .expected_keywords
                        .iter()
                        .all(|kw| lower.contains(&kw.to_lowercase()))
                    {
                        correct += 1;
                    }
                }
                Err(e) => {
                    rate_limited = e.contains("429") || e.contains("Too Many Requests");
                    failed += 1;
                }
            }
        }

        let median_ms = if latencies.is_empty() {
            0
        } else {
            latencies.sort_unstable();
            let n = latencies.len();
            if n % 2 == 1 {
                latencies[n / 2]
            } else {
                (latencies[n / 2 - 1] + latencies[n / 2]) / 2
            }
        };

        rows.push(Row {
            model: model.to_string(),
            correct,
            median_ms,
            failed,
            rate_limited,
        });
    }

    rows.sort_by(|a, b| {
        let a_unavail = a.unavailable();
        let b_unavail = b.unavailable();
        match (a_unavail, b_unavail) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b
                .correct
                .cmp(&a.correct)
                .then_with(|| a.median_ms.cmp(&b.median_ms)),
        }
    });

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAsk;
    static LIMITED_ASKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[async_trait]
    impl Ask for FakeAsk {
        async fn ask(&self, model: &str, prompt: &str) -> Result<(String, u64), String> {
            match model {
                "fast-right" => {
                    let case = CASES.iter().find(|c| prompt.contains(c.question)).unwrap();
                    let answer = case.expected_keywords.join(" ");
                    Ok((answer, 50))
                }
                "slow-right" => {
                    let case = CASES.iter().find(|c| prompt.contains(c.question)).unwrap();
                    let answer = case.expected_keywords.join(" ");
                    Ok((answer, 200))
                }
                "wrong" => Ok(("totally wrong answer".to_string(), 50)),
                "erroring" => Err("connection refused".to_string()),
                "limited" => {
                    LIMITED_ASKED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err("Provider x returned error status 429 Too Many Requests".to_string())
                }
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn test_rank_ordering() {
        let models = vec![
            "slow-right".to_string(),
            "wrong".to_string(),
            "erroring".to_string(),
            "fast-right".to_string(),
        ];

        let rows = rank(&models, FakeAsk).await;

        let names: Vec<&str> = rows.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(names, vec!["fast-right", "slow-right", "wrong", "erroring"]);

        assert_eq!(rows[0].correct, 8);
        assert_eq!(rows[0].median_ms, 50);
        assert_eq!(rows[0].failed, 0);
        assert!(!rows[0].unavailable());

        assert_eq!(rows[1].correct, 8);
        assert_eq!(rows[1].median_ms, 200);
        assert_eq!(rows[1].failed, 0);
        assert!(!rows[1].unavailable());

        assert_eq!(rows[2].correct, 0);
        assert_eq!(rows[2].median_ms, 50);
        assert_eq!(rows[2].failed, 0);
        assert!(!rows[2].unavailable());

        assert_eq!(rows[3].correct, 0);
        assert_eq!(rows[3].failed, 8);
        assert!(rows[3].unavailable());
        assert!(!rows[3].rate_limited);
    }

    /// "Too many requests" once: not asked the rest, and said so.
    #[tokio::test]
    async fn a_rate_limited_model_is_asked_once() {
        let rows = rank(&["limited".to_string()], FakeAsk).await;
        assert_eq!(LIMITED_ASKED.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(rows[0].rate_limited && rows[0].unavailable());
    }
}
