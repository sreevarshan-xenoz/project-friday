use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Mission Scorecard ───────────────────────────────────────────────

/// Scorecard produced after every mission completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionScorecard {
    pub mission_id: String,
    pub goal: String,
    pub success: bool,
    pub duration_ms: u64,
    pub steps_total: usize,
    pub steps_completed: usize,
    pub steps_failed: usize,
    pub retries_used: usize,
    pub recoveries_succeeded: usize,
    pub cost_tokens: u64,
    pub cost_usd: f64,
    pub user_interventions: usize,
    pub reflection_lessons: usize,
}

impl MissionScorecard {
    pub fn completion_rate(&self) -> f64 {
        if self.steps_total == 0 {
            return 0.0;
        }
        self.steps_completed as f64 / self.steps_total as f64
    }

    pub fn failure_rate(&self) -> f64 {
        if self.steps_total == 0 {
            return 0.0;
        }
        self.steps_failed as f64 / self.steps_total as f64
    }

    pub fn cost_efficiency(&self) -> f64 {
        if self.steps_completed == 0 {
            return 0.0;
        }
        self.cost_tokens as f64 / self.steps_completed as f64
    }
}

// ── Metric Value ────────────────────────────────────────────────────

/// A single metric reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricReading {
    pub name: String,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    pub tags: HashMap<String, String>,
}

// ── Trend ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDirection {
    Improving,
    Stable,
    Degrading,
}

/// A weekly/monthly trend for a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub metric: String,
    pub direction: TrendDirection,
    pub current: f64,
    pub previous: f64,
    pub change_pct: f64,
}

// ── Alert ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAlert {
    pub metric: String,
    pub current: f64,
    pub threshold: f64,
    pub severity: AlertSeverity,
    pub timestamp: DateTime<Utc>,
}

// ── Evaluation Event ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvaluationEvent {
    MissionCompleted {
        mission_id: String,
        scorecard: MissionScorecard,
    },
    WeeklyTrend {
        week: String,
        metrics: HashMap<String, f64>,
        trends: HashMap<String, TrendDirection>,
    },
    MetricAlert {
        metric: String,
        current: f64,
        threshold: f64,
        severity: AlertSeverity,
    },
}

// ── Evaluation Areas ────────────────────────────────────────────────

/// The seven evaluation areas from the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvaluationArea {
    Planning,
    Memory,
    ToolUse,
    Learning,
    Autonomy,
    HumanFactors,
    Performance,
}

impl EvaluationArea {
    pub fn all() -> &'static [EvaluationArea] {
        &[
            Self::Planning,
            Self::Memory,
            Self::ToolUse,
            Self::Learning,
            Self::Autonomy,
            Self::HumanFactors,
            Self::Performance,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Memory => "memory",
            Self::ToolUse => "tool_use",
            Self::Learning => "learning",
            Self::Autonomy => "autonomy",
            Self::HumanFactors => "human_factors",
            Self::Performance => "performance",
        }
    }

    pub fn target(&self) -> &'static str {
        match self {
            Self::Planning => "90%+ success rate, <20% replan rate",
            Self::Memory => "Precision >80%, recall latency <50ms",
            Self::ToolUse => "95%+ success rate, <1.5 avg retries",
            Self::Learning => "Mistake reduction >10% week-over-week",
            Self::Autonomy => "80%+ completion without help",
            Self::HumanFactors => ">70% satisfaction, <20% unnecessary interruptions",
            Self::Performance => "<500ms p50, <2GB RAM, <10% CPU idle",
        }
    }
}

// ── Metric Collector ────────────────────────────────────────────────

/// Simple in-memory metric collector.
pub struct MetricCollector {
    readings: Vec<MetricReading>,
}

impl MetricCollector {
    pub fn new() -> Self {
        Self {
            readings: Vec::new(),
        }
    }

    pub fn record(&mut self, name: impl Into<String>, value: f64) {
        self.readings.push(MetricReading {
            name: name.into(),
            value,
            timestamp: Utc::now(),
            tags: HashMap::new(),
        });
    }

    pub fn record_with_tags(
        &mut self,
        name: impl Into<String>,
        value: f64,
        tags: HashMap<String, String>,
    ) {
        self.readings.push(MetricReading {
            name: name.into(),
            value,
            timestamp: Utc::now(),
            tags,
        });
    }

    pub fn latest(&self, name: &str) -> Option<&MetricReading> {
        self.readings.iter().rfind(|r| r.name == name)
    }

    pub fn all_for(&self, name: &str) -> Vec<&MetricReading> {
        self.readings.iter().filter(|r| r.name == name).collect()
    }

    pub fn average(&self, name: &str) -> Option<f64> {
        let values: Vec<f64> = self
            .readings
            .iter()
            .filter(|r| r.name == name)
            .map(|r| r.value)
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values.iter().sum::<f64>() / values.len() as f64)
        }
    }

    pub fn sum(&self, name: &str) -> f64 {
        self.readings
            .iter()
            .filter(|r| r.name == name)
            .map(|r| r.value)
            .sum()
    }

    pub fn count(&self, name: &str) -> usize {
        self.readings.iter().filter(|r| r.name == name).count()
    }

    pub fn clear(&mut self) {
        self.readings.clear();
    }
}

impl Default for MetricCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scorecard_metrics() {
        let scorecard = MissionScorecard {
            mission_id: "m1".into(),
            goal: "test".into(),
            success: true,
            duration_ms: 5000,
            steps_total: 10,
            steps_completed: 8,
            steps_failed: 2,
            retries_used: 1,
            recoveries_succeeded: 1,
            cost_tokens: 1000,
            cost_usd: 0.02,
            user_interventions: 0,
            reflection_lessons: 2,
        };

        assert_eq!(scorecard.completion_rate(), 0.8);
        assert_eq!(scorecard.failure_rate(), 0.2);
        assert_eq!(scorecard.cost_efficiency(), 125.0);
    }

    #[test]
    fn metric_collector_basics() {
        let mut collector = MetricCollector::new();
        collector.record("latency", 100.0);
        collector.record("latency", 200.0);
        collector.record("latency", 150.0);

        assert_eq!(collector.count("latency"), 3);
        assert_eq!(collector.average("latency"), Some(150.0));
        assert_eq!(collector.sum("latency"), 450.0);
        assert_eq!(collector.latest("latency").unwrap().value, 150.0);
        assert!(collector.average("nonexistent").is_none());
    }

    #[test]
    fn evaluation_areas_all_present() {
        assert_eq!(EvaluationArea::all().len(), 7);
    }

    #[test]
    fn trend_direction_serialize() {
        let trend = Trend {
            metric: "test".into(),
            direction: TrendDirection::Improving,
            current: 0.9,
            previous: 0.8,
            change_pct: 12.5,
        };
        let json = serde_json::to_string(&trend).unwrap();
        assert!(json.contains("Improving"));
    }
}
