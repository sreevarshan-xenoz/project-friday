//! Actual cost telemetry for completed model/operator decisions.
//!
//! `CostModel` predicts cost; this ledger records observed usage. Keeping the
//! two separate preserves the immutable decision history while making cost
//! queries available per goal and model.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostEntry {
    pub goal_id: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost: f64,
    pub actual_cost: f64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostLedger {
    entries: Vec<CostEntry>,
}

impl CostLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, entry: CostEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[CostEntry] {
        &self.entries
    }

    pub fn total_for_goal(&self, goal_id: &str) -> f64 {
        self.entries
            .iter()
            .filter(|entry| entry.goal_id == goal_id)
            .map(|entry| entry.actual_cost)
            .sum()
    }

    pub fn total_for_model(&self, model: &str) -> f64 {
        self.entries
            .iter()
            .filter(|entry| entry.model == model)
            .map(|entry| entry.actual_cost)
            .sum()
    }

    pub fn by_model(&self) -> HashMap<String, f64> {
        let mut totals = HashMap::new();
        for entry in &self.entries {
            *totals.entry(entry.model.clone()).or_insert(0.0) += entry.actual_cost;
        }
        totals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(goal_id: &str, model: &str, cost: f64) -> CostEntry {
        CostEntry {
            goal_id: goal_id.into(),
            model: model.into(),
            input_tokens: 10,
            output_tokens: 5,
            estimated_cost: cost,
            actual_cost: cost,
            timestamp_ms: 1,
        }
    }

    #[test]
    fn queries_actual_cost_by_goal_and_model() {
        let mut ledger = CostLedger::new();
        ledger.record(entry("goal-a", "local", 0.01));
        ledger.record(entry("goal-a", "cloud", 0.20));
        ledger.record(entry("goal-b", "local", 0.02));

        assert!((ledger.total_for_goal("goal-a") - 0.21).abs() < f64::EPSILON);
        assert!((ledger.total_for_model("local") - 0.03).abs() < f64::EPSILON);
        assert_eq!(ledger.by_model().len(), 2);
    }

    #[test]
    fn ledger_round_trips_for_persistence() {
        let mut ledger = CostLedger::new();
        ledger.record(entry("goal-a", "local", 0.01));
        let encoded = serde_json::to_vec(&ledger).unwrap();
        let decoded: CostLedger = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.entries(), ledger.entries());
    }
}
