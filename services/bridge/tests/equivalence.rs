//! Incremental vs point-in-time equivalence suite.
//!
//! ADR-0005 mechanism C4+C6 replaces the O(history) `regenerate()` +
//! `build_all()` + `summarize()` cycle with an incremental `TimelineStore`
//! that appends one point-in-time snapshot per decision. This suite pins the
//! equivalence gate at the **wire level**: the DTOs the bridge actually
//! serializes must be identical whether they come from the incremental store
//! or from a pure point-in-time replay (`TimelineBuilder::build_all_pti`).
//!
//! Semantic equality (`may differ`): `ReputationDto.last_updated_ms` is wall
//! clock by design, so it is normalized away before comparing.

use std::sync::Arc;

use friday_bridge::protocol::DecisionTimelineDto;
use friday_experience::{
    CalibrationStore, ExperienceStore, ReputationStore, TimelineBuilder, TimelineStore,
};
use friday_plugin_runtime::{
    Capability, CorrelationId, DecisionLedger, DecisionRecord, ExecutionPlan, PlanOutcome,
    PlanningRationale, SelectionContext,
};

fn make_record(cap: Capability, operator: &str, success: bool) -> DecisionRecord {
    let cid = CorrelationId::new();
    DecisionRecord {
        id: cid.clone(),
        timestamp_ms: 1_784_000_000_000,
        capability: cap,
        context: SelectionContext::default(),
        parameters: Default::default(),
        plan: ExecutionPlan {
            plan_id: cid.clone(),
            version: 1,
            capability: Capability::ReadFile,
            operator_id: operator.into(),
            estimated_latency_ms: 10,
            estimated_cost: 0.1,
            estimated_success_probability: 0.9,
            estimated_risk: 0.1,
            confidence: 0.9,
            fallback_chain: Vec::new(),
            rationale: PlanningRationale {
                candidates: Vec::new(),
                selected_operator_id: operator.into(),
                composite_score: 0.9,
                explanation: "selected".into(),
                correlation_id: cid,
                evaluator_evidence: Vec::new(),
            },
        },
        outcome: PlanOutcome {
            latency_ms: 10,
            cost: 0.1,
            success,
            error: if success {
                None
            } else {
                Some("timeout".into())
            },
            completed_steps: if success { 1 } else { 0 },
            cancelled: false,
            timeout: !success,
            retries: 0,
        },
        reflection: None,
    }
}

fn normalize_wall_clock(t: &mut DecisionTimelineDto) {
    if let Some(r) = t.reputation.as_mut() {
        r.last_updated_ms = 0;
    }
    t.feedback.timestamp_ms = 0;
}

#[test]
fn incremental_timeline_store_matches_pti_rebuild_at_wire_level() {
    let mut ledger = DecisionLedger::new();
    for i in 0..40 {
        let cap = if i % 2 == 0 {
            Capability::ReadFile
        } else {
            Capability::WriteFile
        };
        let op = if i % 3 == 0 { "fs-native" } else { "shell" };
        ledger.append(make_record(cap, op, i % 5 != 0));
    }

    let mut store = TimelineStore::new(CalibrationStore::new());
    for i in 0..ledger.len() {
        let shared = ledger.get_shared(i).expect("record present");
        store.append(Arc::clone(&shared));
    }

    let incremental: Vec<DecisionTimelineDto> = store
        .timelines()
        .iter()
        .map(DecisionTimelineDto::from)
        .collect();

    let empty_e = ExperienceStore::new();
    let empty_r = ReputationStore::new();
    let empty_c = CalibrationStore::new();
    let pti: Vec<DecisionTimelineDto> = TimelineBuilder::new(&ledger, &empty_e, &empty_r, &empty_c)
        .build_all_pti()
        .iter()
        .map(DecisionTimelineDto::from)
        .collect();

    assert_eq!(incremental.len(), pti.len());
    for (inc, ref_) in incremental.iter().zip(pti.iter()) {
        let mut inc = inc.clone();
        let mut ref_ = ref_.clone();
        normalize_wall_clock(&mut inc);
        normalize_wall_clock(&mut ref_);
        assert_eq!(inc, ref_);
    }
}
