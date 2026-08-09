use crate::handler::PhaseHandler;
use crate::phase::CognitivePhase;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of phase handlers. The runtime queries this to find
/// which engines participate in each phase.
///
/// The runtime doesn't know any engine names.
/// It only knows: "Who wants to participate in this phase?"
pub struct PhaseRegistry {
    /// phase → list of handlers
    handlers: HashMap<CognitivePhase, Vec<Arc<dyn PhaseHandler>>>,
}

impl PhaseRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for its declared phases.
    pub fn register(&mut self, handler: Arc<dyn PhaseHandler>) {
        for phase in handler.phases() {
            self.handlers
                .entry(phase)
                .or_default()
                .push(handler.clone());
        }
    }

    /// Get all handlers for a given phase.
    pub fn handlers_for(&self, phase: CognitivePhase) -> Vec<Arc<dyn PhaseHandler>> {
        self.handlers.get(&phase).cloned().unwrap_or_default()
    }

    /// Total number of registered handlers (counting each phase separately).
    pub fn count(&self) -> usize {
        self.handlers.values().map(|v| v.len()).sum()
    }

    /// Number of unique handler instances.
    pub fn unique_count(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for handlers in self.handlers.values() {
            for h in handlers {
                seen.insert(h.name());
            }
        }
        seen.len()
    }

    /// All phases that have at least one handler.
    pub fn active_phases(&self) -> Vec<CognitivePhase> {
        self.handlers
            .keys()
            .filter(|p| !self.handlers[*p].is_empty())
            .copied()
            .collect()
    }
}

impl Default for PhaseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::{PhaseHandler, PhaseResult};
    use crate::workspace::GlobalWorkspace;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct NopHandler {
        name: String,
        phases: Vec<CognitivePhase>,
    }

    #[async_trait]
    impl PhaseHandler for NopHandler {
        fn name(&self) -> &str {
            &self.name
        }
        fn phases(&self) -> Vec<CognitivePhase> {
            self.phases.clone()
        }
        async fn execute(
            &self,
            _phase: CognitivePhase,
            _workspace: &GlobalWorkspace,
            _tick_number: u64,
        ) -> PhaseResult {
            PhaseResult::Ok
        }
    }

    #[test]
    fn register_and_query() {
        let mut reg = PhaseRegistry::new();
        reg.register(Arc::new(NopHandler {
            name: "test_observe".into(),
            phases: vec![CognitivePhase::Observe],
        }));
        reg.register(Arc::new(NopHandler {
            name: "test_plan".into(),
            phases: vec![CognitivePhase::Plan, CognitivePhase::Act],
        }));

        assert_eq!(reg.handlers_for(CognitivePhase::Observe).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Plan).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Act).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Learn).len(), 0);
        assert_eq!(reg.unique_count(), 2);
    }

    #[test]
    fn multi_phase_handler() {
        let mut reg = PhaseRegistry::new();
        reg.register(Arc::new(NopHandler {
            name: "multi".into(),
            phases: vec![
                CognitivePhase::Observe,
                CognitivePhase::Understand,
                CognitivePhase::Learn,
            ],
        }));

        assert_eq!(reg.handlers_for(CognitivePhase::Observe).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Understand).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Learn).len(), 1);
        assert_eq!(reg.handlers_for(CognitivePhase::Plan).len(), 0);
    }
}
