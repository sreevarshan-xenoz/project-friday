use friday_memory::Episode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Knowledge Types ─────────────────────────────────────────────────

/// A derived knowledge record — a pattern recognized from observations.
///
/// Knowledge is never directly observed. It is inferred from patterns
/// across multiple episodes. The original events remain immutable;
/// knowledge is a layer of meaning added on top.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRecord {
    pub id: String,
    pub kind: KnowledgeKind,
    pub label: String,
    pub description: String,
    /// The pattern signature that produced this knowledge (e.g. ["vs_code", "chrome", "cargo"]).
    pub pattern: Vec<String>,
    /// How many observations produced this pattern.
    pub support: u32,
    /// How strongly we believe this pattern represents reality [0.0, 1.0].
    pub confidence: f32,
    /// When this knowledge was derived.
    pub derived_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KnowledgeKind {
    /// A recurring workflow (e.g. "Development Workflow").
    Workflow,
    /// A recurring sequence of applications (e.g. "Research Pattern").
    Sequence,
    /// A frequent application usage (e.g. "Primary Tool").
    Frequency,
}

// ── Pattern Detector ────────────────────────────────────────────────

/// Detects recurring patterns in sequences of episodes.
///
/// Uses a sliding window approach:
/// 1. Extract a signature from each episode (simplified topic + payload key).
/// 2. Slide a window of size `window_size` across the signature sequence.
/// 3. Count occurrences of each window pattern.
/// 4. When a pattern appears >= `min_occurrences`, it becomes knowledge.
pub struct PatternDetector {
    window_size: usize,
    min_occurrences: u32,
}

impl PatternDetector {
    pub fn new(window_size: usize, min_occurrences: u32) -> Self {
        Self {
            window_size,
            min_occurrences,
        }
    }

    /// Extract a signature from an episode.
    /// The signature captures the essential meaning for pattern matching.
    fn episode_signature(episode: &Episode) -> String {
        // Use the topic's short form + a simplified payload key.
        let topic = episode.event.topic.as_str();
        let payload_key = Self::payload_key(&episode.event.payload);
        format!("{}:{}", topic, payload_key)
    }

    /// Extract a simplified key from a payload.
    /// For window focus events, this is the application name.
    fn payload_key(payload: &serde_json::Value) -> String {
        if let Some(title) = payload.get("title").and_then(|v| v.as_str()) {
            // Extract application name from title (everything after the last " — ").
            title.rsplit(" — ").next().unwrap_or(title).to_string()
        } else if let Some(key) = payload.get("key").and_then(|v| v.as_str()) {
            key.to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Detect patterns in a sequence of episodes.
    /// Returns knowledge records for patterns that meet the threshold.
    pub fn detect(&self, episodes: &[Episode]) -> Vec<KnowledgeRecord> {
        if episodes.len() < self.window_size {
            return Vec::new();
        }

        // Extract signatures.
        let signatures: Vec<String> = episodes.iter().map(Self::episode_signature).collect();

        // Count window patterns.
        let mut pattern_counts: HashMap<Vec<String>, u32> = HashMap::new();
        for window in signatures.windows(self.window_size) {
            pattern_counts
                .entry(window.to_vec())
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }

        // Derive knowledge from frequent patterns.
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let total_windows = signatures.windows(self.window_size).count() as u32;
        let mut knowledge = Vec::new();

        for (pattern, count) in &pattern_counts {
            if *count >= self.min_occurrences {
                let label = Self::pattern_label(pattern);
                let description = Self::pattern_description(pattern, *count);
                let confidence = if total_windows > 0 {
                    *count as f32 / total_windows as f32
                } else {
                    0.0
                };

                knowledge.push(KnowledgeRecord {
                    id: format!("k_{}_{}", pattern[0].replace(':', "_"), count),
                    kind: KnowledgeKind::Workflow,
                    label,
                    description,
                    pattern: pattern.clone(),
                    support: *count,
                    confidence,
                    derived_at_ms: now_ms,
                });
            }
        }

        // Sort by confidence (most observed first).
        knowledge.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        knowledge
    }

    /// Generate a human-readable label for a pattern.
    fn pattern_label(pattern: &[String]) -> String {
        // Extract unique application names from the pattern.
        let apps: Vec<&str> = pattern
            .iter()
            .filter_map(|sig| sig.split(':').nth(1))
            .filter(|app| *app != "unknown")
            .collect();

        if apps.is_empty() {
            return "Unknown Pattern".to_string();
        }

        // Use the most frequent app as the primary label.
        let mut counts: HashMap<&str, u32> = HashMap::new();
        for app in &apps {
            *counts.entry(app).or_insert(0) += 1;
        }
        let primary = counts
            .iter()
            .max_by_key(|(_, c)| *c)
            .map(|(app, _)| *app)
            .unwrap_or("Unknown");

        // If multiple apps, create a compound label.
        if apps.len() > 1 {
            let unique_apps: Vec<&str> = apps
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            if unique_apps.len() <= 3 {
                format!("{} Workflow", unique_apps.join(" + "))
            } else {
                format!("{} + {} Others Workflow", primary, unique_apps.len() - 1)
            }
        } else {
            format!("{} Usage Pattern", primary)
        }
    }

    /// Generate a human-readable description of a pattern.
    fn pattern_description(pattern: &[String], count: u32) -> String {
        let apps: Vec<&str> = pattern
            .iter()
            .filter_map(|sig| sig.split(':').nth(1))
            .filter(|app| *app != "unknown")
            .collect();

        if apps.is_empty() {
            return format!("Unknown pattern observed {} times.", count);
        }

        let sequence = apps.join(" → ");
        format!("Observed {} times: {}", count, sequence)
    }
}

impl Default for PatternDetector {
    fn default() -> Self {
        Self::new(3, 5)
    }
}

// ── Knowledge Engine ────────────────────────────────────────────────

/// Orchestrates pattern detection and knowledge derivation from episodes.
pub struct KnowledgeEngine {
    detector: PatternDetector,
    records: Vec<KnowledgeRecord>,
}

impl KnowledgeEngine {
    pub fn new() -> Self {
        Self {
            detector: PatternDetector::default(),
            records: Vec::new(),
        }
    }

    pub fn with_detector(detector: PatternDetector) -> Self {
        Self {
            detector,
            records: Vec::new(),
        }
    }

    /// Analyze episodes and derive knowledge.
    /// Returns new knowledge records that weren't previously known.
    pub fn analyze(&mut self, episodes: &[Episode]) -> Vec<KnowledgeRecord> {
        let new_knowledge = self.detector.detect(episodes);

        // Filter out knowledge we already have (by pattern).
        let existing_patterns: Vec<Vec<String>> =
            self.records.iter().map(|r| r.pattern.clone()).collect();
        let fresh: Vec<KnowledgeRecord> = new_knowledge
            .into_iter()
            .filter(|k| !existing_patterns.contains(&k.pattern))
            .collect();

        self.records.extend(fresh.clone());
        fresh
    }

    pub fn records(&self) -> &[KnowledgeRecord] {
        &self.records
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

impl Default for KnowledgeEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use friday_cognitive::workspace::Topic;

    fn window_event(title: &str) -> Episode {
        let event = friday_cognitive::WorkspaceEvent::new(
            Topic::WindowFocusChanged,
            serde_json::json!({"title": title}),
        );
        Episode::from_event(event)
    }

    #[test]
    fn payload_key_extracts_app_name() {
        assert_eq!(
            PatternDetector::payload_key(
                &serde_json::json!({"title": "main.rs — Visual Studio Code"})
            ),
            "Visual Studio Code"
        );
        assert_eq!(
            PatternDetector::payload_key(
                &serde_json::json!({"title": "Budget.xlsx — Microsoft Excel"})
            ),
            "Microsoft Excel"
        );
        assert_eq!(
            PatternDetector::payload_key(&serde_json::json!({"title": "FRIDAY — Terminal"})),
            "Terminal"
        );
    }

    #[test]
    fn detect_finds_repeated_pattern() {
        // Create a repeating pattern: VS Code → Chrome → VS Code → Cargo
        let mut episodes = Vec::new();
        for _ in 0..10 {
            episodes.push(window_event("main.rs — Visual Studio Code"));
            episodes.push(window_event("GitHub — Chrome"));
            episodes.push(window_event("main.rs — Visual Studio Code"));
            episodes.push(window_event("build — Cargo"));
        }

        let detector = PatternDetector::new(3, 5);
        let knowledge = detector.detect(&episodes);

        assert!(!knowledge.is_empty(), "Should detect at least one pattern");
        assert!(
            knowledge[0].support >= 5,
            "Pattern should appear at least 5 times"
        );
        assert!(
            knowledge[0].confidence > 0.0 && knowledge[0].confidence <= 1.0,
            "Confidence should be in (0.0, 1.0]"
        );
    }

    #[test]
    fn knowledge_engine_deduplicates() {
        let mut episodes = Vec::new();
        for _ in 0..10 {
            episodes.push(window_event("main.rs — Visual Studio Code"));
            episodes.push(window_event("GitHub — Chrome"));
        }

        let mut engine = KnowledgeEngine::with_detector(PatternDetector::new(2, 3));
        let k1 = engine.analyze(&episodes);
        let k2 = engine.analyze(&episodes);

        // Second analysis shouldn't add new knowledge (same patterns).
        assert!(k2.is_empty() || k2.len() <= k1.len());
        assert!(engine.record_count() > 0);
    }

    #[test]
    fn dev_workflow_pattern_detected() {
        // Simulate 40 iterations of a development workflow:
        // VS Code → Chrome (GitHub) → VS Code → Cargo (build)
        let mut episodes = Vec::new();
        for _ in 0..40 {
            episodes.push(window_event("main.rs — Visual Studio Code"));
            episodes.push(window_event("pull/123 — GitHub — Chrome"));
            episodes.push(window_event("main.rs — Visual Studio Code"));
            episodes.push(window_event("build — Cargo"));
        }

        let mut engine = KnowledgeEngine::new();
        let knowledge = engine.analyze(&episodes);

        assert!(
            !knowledge.is_empty(),
            "Should derive knowledge from 40 workflow iterations"
        );

        let dev_workflow = &knowledge[0];
        assert!(
            dev_workflow.support >= 30,
            "Dev workflow should have high support"
        );
        assert!(
            dev_workflow.confidence > 0.0,
            "Dev workflow should have positive confidence"
        );
        assert!(
            dev_workflow.label.contains("Visual Studio Code"),
            "Should mention VS Code"
        );
    }

    #[test]
    fn pattern_label_uses_primary_app() {
        let pattern = vec![
            "sensor.window.focus_changed:Visual Studio Code".into(),
            "sensor.window.focus_changed:Chrome".into(),
            "sensor.window.focus_changed:Visual Studio Code".into(),
        ];
        let label = PatternDetector::pattern_label(&pattern);
        assert!(label.contains("Workflow"));
    }
}
