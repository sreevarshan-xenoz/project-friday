use std::sync::Arc;

use crate::contract::DecisionRecord;

/// An append-only journal of every planning-and-execution cycle.
///
/// The ledger owns history, not intelligence. It provides only the
/// operations needed to store and replay decisions:
///   - append a record
///   - get by index
///   - iterate all records
///   - query length
///   - clear (for testing)
///
/// Every record is guaranteed to have a corresponding plan and outcome.
/// No execution is ever forgotten.
///
/// Records are stored behind `Arc` so immutable read models (e.g. a
/// `DecisionTimeline`'s point-in-time snapshot) can share the exact record
/// allocation the ledger owns instead of deep-cloning it. The public API is
/// unchanged; `get`/`iter` still hand out `&DecisionRecord`.
#[derive(Debug)]
pub struct DecisionLedger {
    records: Vec<Arc<DecisionRecord>>,
}

impl DecisionLedger {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Append a decision record. This is the only way to add records.
    pub fn append(&mut self, record: DecisionRecord) {
        self.records.push(Arc::new(record));
    }

    /// Append an already-shared record. Callers that keep their own `Arc` can
    /// hand it to the ledger so both sides reference the same allocation.
    pub fn append_shared(&mut self, record: Arc<DecisionRecord>) {
        self.records.push(record);
    }

    /// Get a record by index. Returns None if out of bounds.
    pub fn get(&self, index: usize) -> Option<&DecisionRecord> {
        self.records.get(index).map(|r| r.as_ref())
    }

    /// Get a shared handle to the record at `index`.
    pub fn get_shared(&self, index: usize) -> Option<Arc<DecisionRecord>> {
        self.records.get(index).cloned()
    }

    /// Iterate over all records.
    pub fn iter(&self) -> impl Iterator<Item = &DecisionRecord> {
        self.records.iter().map(|r| r.as_ref())
    }

    /// Number of records in the ledger.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns true if the ledger is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Clear all records. Intended for testing.
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

impl Default for DecisionLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for DecisionLedger {
    type Item = DecisionRecord;
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.records.into_iter(),
        }
    }
}

/// Consuming iterator over a `DecisionLedger`. Yields owned records: when the
/// last `Arc` reference is unique (the usual case for a consumed ledger) the
/// record is moved out untouched; otherwise it is cloned.
pub struct IntoIter {
    inner: std::vec::IntoIter<Arc<DecisionRecord>>,
}

impl Iterator for IntoIter {
    type Item = DecisionRecord;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|r| Arc::try_unwrap(r).unwrap_or_else(|r| (*r).clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        Capability, ExecutionPlan, PlanOutcome, PlanningRationale,
        SelectionContext, CorrelationId,
    };
    use std::collections::HashMap;
    use crate::utils::now_ms;

    fn sample_record() -> DecisionRecord {
        let cid = CorrelationId::new();
        DecisionRecord {
            id: cid.clone(),
            timestamp_ms: now_ms(),
            capability: Capability::ReadFile,
            context: SelectionContext::default(),
            parameters: HashMap::new(),
            plan: ExecutionPlan {
                plan_id: cid.clone(),
                version: 1,
                capability: Capability::ReadFile,
                operator_id: "fs-native".into(),
                estimated_latency_ms: 10,
                estimated_cost: 0.1,
                estimated_success_probability: 0.95,
                estimated_risk: 0.05,
                confidence: 0.95,
                fallback_chain: Vec::new(),
                rationale: PlanningRationale {
                    candidates: Vec::new(),
                    selected_operator_id: "fs-native".into(),
                    composite_score: 0.95,
                    explanation: "selected fs-native".into(),
                    correlation_id: cid,
                    evaluator_evidence: Vec::new(),
                },
            },
            outcome: PlanOutcome {
                latency_ms: 10,
                cost: 0.1,
                success: true,
                error: None,
                completed_steps: 1,
                cancelled: false,
                timeout: false,
                retries: 0,
            },
            reflection: None,
        }
    }

    #[test]
    fn new_ledger_is_empty() {
        let ledger = DecisionLedger::new();
        assert!(ledger.is_empty());
        assert_eq!(ledger.len(), 0);
    }

    #[test]
    fn append_increments_length() {
        let mut ledger = DecisionLedger::new();
        ledger.append(sample_record());
        assert_eq!(ledger.len(), 1);
        ledger.append(sample_record());
        assert_eq!(ledger.len(), 2);
    }

    #[test]
    fn get_returns_record_by_index() {
        let mut ledger = DecisionLedger::new();
        let rec = sample_record();
        let id = rec.id.clone();
        ledger.append(rec);
        let retrieved = ledger.get(0).unwrap();
        assert_eq!(retrieved.id, id);
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let ledger = DecisionLedger::new();
        assert!(ledger.get(0).is_none());
        assert!(ledger.get(100).is_none());
    }

    #[test]
    fn iter_traverses_all_records() {
        let mut ledger = DecisionLedger::new();
        ledger.append(sample_record());
        ledger.append(sample_record());
        let count = ledger.iter().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn clear_removes_all_records() {
        let mut ledger = DecisionLedger::new();
        ledger.append(sample_record());
        ledger.append(sample_record());
        ledger.clear();
        assert!(ledger.is_empty());
    }

    #[test]
    fn into_iter_consumes_ledger() {
        let mut ledger = DecisionLedger::new();
        ledger.append(sample_record());
        ledger.append(sample_record());
        let count = ledger.into_iter().count();
        assert_eq!(count, 2);
    }
}
