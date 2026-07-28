use friday_knowledge::KnowledgeRecord;
use serde::{Deserialize, Serialize};

// ── Reflection Types ────────────────────────────────────────────────

/// An outcome — something that happened in the world.
///
/// Outcomes are observed, not derived. They represent the results
/// of goals, tasks, or actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    pub kind: OutcomeKind,
    pub context: String,
    pub detail: serde_json::Value,
    pub occurred_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutcomeKind {
    MissionSucceeded,
    MissionFailed,
    TaskCompleted,
    TaskAbandoned,
    Error,
    UnexpectedSuccess,
    UnexpectedFailure,
}

/// An insight — a semantic statement derived from knowledge and outcomes.
///
/// Insights are the output of reflection. They encode understanding
/// about what happened and why, grounded in observed patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub id: String,
    pub kind: InsightKind,
    /// The knowledge record that produced this insight.
    pub knowledge_id: String,
    /// The outcome that triggered this reflection.
    pub outcome_id: String,
    /// Human-readable insight statement.
    pub statement: String,
    /// How strongly we believe this insight [0.0, 1.0].
    pub confidence: f32,
    /// When this insight was derived.
    pub derived_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InsightKind {
    /// Pattern worked as expected — reinforces existing knowledge.
    Reinforcement,
    /// Pattern failed — something went wrong.
    Lesson,
    /// Pattern produced an unexpected result — needs investigation.
    Warning,
    /// Pattern could be optimized — there's a better way.
    Optimization,
}

// ── Reflection Engine ───────────────────────────────────────────────

/// Derives insights from knowledge records and outcomes.
///
/// Reflection is outcome-agnostic. It produces different kinds of insights
/// depending on what happened:
///
/// - Success → Reinforcement
/// - Failure → Lesson
/// - Unexpected → Warning
///
/// This keeps the architecture clean:
///
/// ```text
/// Workspace → Memory → Knowledge → Reflection → Learning
/// ```
pub struct ReflectionEngine {
    insights: Vec<Insight>,
}

impl ReflectionEngine {
    pub fn new() -> Self {
        Self {
            insights: Vec::new(),
        }
    }

    /// Reflect on a knowledge record and an outcome to derive an insight.
    pub fn reflect(&mut self, knowledge: &KnowledgeRecord, outcome: &Outcome) -> Option<Insight> {
        let kind = self.classify_outcome(outcome);
        let statement = self.derive_statement(knowledge, outcome, kind);
        let confidence = self.compute_confidence(knowledge, outcome, kind);

        let insight = Insight {
            id: format!("i_{}_{}", knowledge.id, outcome.id),
            kind,
            knowledge_id: knowledge.id.clone(),
            outcome_id: outcome.id.clone(),
            statement,
            confidence,
            derived_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        };

        self.insights.push(insight.clone());
        Some(insight)
    }

    /// Reflect on multiple knowledge records against a single outcome.
    pub fn reflect_all(
        &mut self,
        knowledge_records: &[KnowledgeRecord],
        outcome: &Outcome,
    ) -> Vec<Insight> {
        knowledge_records
            .iter()
            .filter_map(|k| self.reflect(k, outcome))
            .collect()
    }

    /// Classify an outcome into an insight kind.
    fn classify_outcome(&self, outcome: &Outcome) -> InsightKind {
        match outcome.kind {
            OutcomeKind::MissionSucceeded | OutcomeKind::TaskCompleted => InsightKind::Reinforcement,
            OutcomeKind::MissionFailed | OutcomeKind::TaskAbandoned => InsightKind::Lesson,
            OutcomeKind::Error => InsightKind::Lesson,
            OutcomeKind::UnexpectedSuccess => InsightKind::Optimization,
            OutcomeKind::UnexpectedFailure => InsightKind::Warning,
        }
    }

    /// Derive a human-readable insight statement.
    fn derive_statement(&self, knowledge: &KnowledgeRecord, outcome: &Outcome, kind: InsightKind) -> String {
        let apps: Vec<&str> = knowledge
            .pattern
            .iter()
            .filter_map(|sig| sig.split(':').nth(1))
            .filter(|app| *app != "unknown")
            .collect();

        let pattern_desc = if apps.len() >= 2 {
            format!("{} usually follows {}", apps[0], apps[1])
        } else if apps.len() == 1 {
            format!("{} is frequently used", apps[0])
        } else {
            "A pattern was observed".to_string()
        };

        match kind {
            InsightKind::Reinforcement => {
                format!(
                    "Pattern '{}' confirmed during '{}'. {} (support: {}).",
                    knowledge.label, outcome.context, pattern_desc, knowledge.support
                )
            }
            InsightKind::Lesson => {
                format!(
                    "Mission '{}' failed. Observed pattern: {}. Confidence: {:.0}%.",
                    outcome.context, pattern_desc, knowledge.confidence * 100.0
                )
            }
            InsightKind::Warning => {
                format!(
                    "Unexpected failure during '{}'. Pattern '{}' (support: {}) may be relevant.",
                    outcome.context, knowledge.label, knowledge.support
                )
            }
            InsightKind::Optimization => {
                format!(
                    "Unexpected success during '{}'. Pattern '{}' may have contributed (confidence: {:.0}%).",
                    outcome.context, knowledge.label, knowledge.confidence * 100.0
                )
            }
        }
    }

    /// Compute insight confidence based on knowledge confidence, outcome severity, and insight kind.
    fn compute_confidence(&self, knowledge: &KnowledgeRecord, outcome: &Outcome, kind: InsightKind) -> f32 {
        let base = knowledge.confidence;
        let severity = match kind {
            InsightKind::Lesson => match outcome.kind {
                OutcomeKind::MissionFailed => 1.0,
                OutcomeKind::TaskAbandoned => 0.8,
                OutcomeKind::Error => 0.9,
                _ => 0.5,
            },
            InsightKind::Warning => 0.9,
            InsightKind::Optimization => 0.7,
            InsightKind::Reinforcement => 0.6,
        };
        (base * severity).clamp(0.0, 1.0)
    }

    pub fn insights(&self) -> &[Insight] {
        &self.insights
    }

    pub fn insight_count(&self) -> usize {
        self.insights.len()
    }

    pub fn insights_for_knowledge(&self, knowledge_id: &str) -> Vec<&Insight> {
        self.insights
            .iter()
            .filter(|i| i.knowledge_id == knowledge_id)
            .collect()
    }

    pub fn insights_of_kind(&self, kind: InsightKind) -> Vec<&Insight> {
        self.insights.iter().filter(|i| i.kind == kind).collect()
    }
}

impl Default for ReflectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use friday_knowledge::KnowledgeKind;

    fn test_knowledge(label: &str, pattern: Vec<&str>, support: u32, confidence: f32) -> KnowledgeRecord {
        KnowledgeRecord {
            id: format!("k_{}", label.replace(' ', "_")),
            kind: KnowledgeKind::Workflow,
            label: label.to_string(),
            description: format!("Test knowledge: {}", label),
            pattern: pattern.into_iter().map(String::from).collect(),
            support,
            confidence,
            derived_at_ms: 1000,
        }
    }

    fn test_outcome(kind: OutcomeKind, context: &str) -> Outcome {
        Outcome {
            id: format!("o_{}", uuid::Uuid::new_v4()),
            kind,
            context: context.to_string(),
            detail: serde_json::json!({}),
            occurred_at_ms: 2000,
        }
    }

    #[test]
    fn failure_creates_lesson() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("Dev Workflow", vec!["vs_code", "chrome"], 37, 0.25);
        let outcome = test_outcome(OutcomeKind::MissionFailed, "deploy");

        let insight = engine.reflect(&k, &outcome).unwrap();
        assert_eq!(insight.kind, InsightKind::Lesson);
        assert!(insight.statement.contains("deploy"));
    }

    #[test]
    fn success_creates_reinforcement() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("Dev Workflow", vec!["vs_code", "chrome"], 37, 0.25);
        let outcome = test_outcome(OutcomeKind::MissionSucceeded, "deploy");

        let insight = engine.reflect(&k, &outcome).unwrap();
        assert_eq!(insight.kind, InsightKind::Reinforcement);
        assert!(insight.statement.contains("confirmed"));
    }

    #[test]
    fn unexpected_failure_creates_warning() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("Dev Workflow", vec!["vs_code", "chrome"], 37, 0.25);
        let outcome = test_outcome(OutcomeKind::UnexpectedFailure, "build");

        let insight = engine.reflect(&k, &outcome).unwrap();
        assert_eq!(insight.kind, InsightKind::Warning);
    }

    #[test]
    fn unexpected_success_creates_optimization() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("Dev Workflow", vec!["vs_code", "chrome"], 37, 0.25);
        let outcome = test_outcome(OutcomeKind::UnexpectedSuccess, "build");

        let insight = engine.reflect(&k, &outcome).unwrap();
        assert_eq!(insight.kind, InsightKind::Optimization);
    }

    #[test]
    fn insight_confidence_is_bounded() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("W", vec!["a", "b"], 10, 0.8);

        let kinds = vec![
            OutcomeKind::MissionFailed,
            OutcomeKind::MissionSucceeded,
            OutcomeKind::UnexpectedFailure,
            OutcomeKind::UnexpectedSuccess,
            OutcomeKind::TaskAbandoned,
            OutcomeKind::Error,
        ];

        for kind in kinds {
            let outcome = test_outcome(kind, "test");
            let insight = engine.reflect(&k, &outcome).unwrap();
            assert!(insight.confidence >= 0.0 && insight.confidence <= 1.0,
                "Confidence {} should be in [0.0, 1.0]", insight.confidence);
        }
    }

    #[test]
    fn insights_filter_by_kind() {
        let mut engine = ReflectionEngine::new();
        let k = test_knowledge("W", vec!["a", "b"], 10, 0.5);

        engine.reflect(&k, &test_outcome(OutcomeKind::MissionFailed, "t1"));
        engine.reflect(&k, &test_outcome(OutcomeKind::MissionSucceeded, "t2"));
        engine.reflect(&k, &test_outcome(OutcomeKind::MissionFailed, "t3"));

        let lessons = engine.insights_of_kind(InsightKind::Lesson);
        assert_eq!(lessons.len(), 2);
        let reinforcements = engine.insights_of_kind(InsightKind::Reinforcement);
        assert_eq!(reinforcements.len(), 1);
    }
}
