use crate::confidence::Confidence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    pub context: Option<String>,
    pub created_at_ms: u64,
    pub confidence: Confidence,
}

impl Goal {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: format!("goal_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            context: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            confidence: Confidence::medium(),
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_creation() {
        let g = Goal::new("test goal");
        assert_eq!(g.description, "test goal");
        assert!(g.id.starts_with("goal_"));
        assert!(g.created_at_ms > 0);
        assert!((g.confidence.value() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn goal_with_context() {
        let g = Goal::new("test").with_context("ctx");
        assert_eq!(g.context, Some("ctx".into()));
    }
}
