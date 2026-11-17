//! Which letters spell which phoneme (a port of dizione's `align.py`, run time
//! only): every phoneme of a pronunciation gets a run of zero to four letters
//! of the word, in order, so that the runs spell the word.

use std::collections::HashMap;

const MAX_RUN: usize = 4;
const UNSEEN: f64 = -14.0;

/// ARPABET symbol without its stress digit.
pub fn base(symbol: &str) -> &str {
    symbol.trim_end_matches(['0', '1', '2'])
}

pub struct Aligner {
    table: HashMap<String, HashMap<String, f64>>,
}

impl Aligner {
    pub fn new(table: HashMap<String, HashMap<String, f64>>) -> Self {
        Self { table }
    }

    fn score(&self, phone: &str, run: &str) -> f64 {
        match self.table.get(phone).and_then(|known| known.get(run)) {
            Some(v) => *v,
            None => UNSEEN - 2.0 * (run.chars().count() as f64 - 1.0).abs(),
        }
    }

    /// Splits `word` into one run of letters per phoneme (`phones` may carry
    /// stress digits). `None` when the word is too long for the pronunciation.
    pub fn align(&self, word: &str, phones: &[String]) -> Option<Vec<String>> {
        let phones: Vec<&str> = phones.iter().map(|p| base(p)).collect();
        let chars: Vec<char> = word.chars().collect();
        let (letters, count) = (chars.len(), phones.len());
        if count == 0 || letters > MAX_RUN * count {
            return None;
        }
        let low = -1e18;
        let mut best = vec![vec![low; letters + 1]; count + 1];
        let mut step = vec![vec![0usize; letters + 1]; count + 1];
        best[0][0] = 0.0;
        for j in 1..=count {
            let phone = phones[j - 1];
            for i in 0..=letters {
                for k in 0..=MAX_RUN.min(i) {
                    if best[j - 1][i - k] > low {
                        let run: String = chars[i - k..i].iter().collect();
                        let value = best[j - 1][i - k] + self.score(phone, &run);
                        if value > best[j][i] {
                            best[j][i] = value;
                            step[j][i] = k;
                        }
                    }
                }
            }
        }
        if best[count][letters] <= low {
            return None;
        }
        let mut runs = Vec::with_capacity(count);
        let mut i = letters;
        for j in (1..=count).rev() {
            let k = step[j][i];
            runs.push(chars[i - k..i].iter().collect::<String>());
            i -= k;
        }
        runs.reverse();
        Some(runs)
    }
}
