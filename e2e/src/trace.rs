use friday_context::AgentContext;
use friday_intelligence::LlmResponse;
use friday_planner::TaskGraph;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ============================================================================
// TEST INFRASTRUCTURE — NOT the FRIDAY Cognitive Trace v1 (friday_core::trace).
// ----------------------------------------------------------------------------
// `ExecutionTrace` / `TraceReplayEngine` capture RAW vertical-slice I/O
// (compiled prompt, raw + parsed LLM response, task graph, durations, errors)
// for the e2e harness's save/load replay. The v1 contract intentionally does
// NOT capture raw prompts/responses. Classified KEEP (test infrastructure).
// Do not merge into v1. See docs/architecture/cognitive-trace-v1.md.
// ============================================================================

/// Capture all telemetry of a single vertical stack execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub goal: String,
    pub context: AgentContext,
    pub compiled_prompt: String,
    pub raw_llm_response: String,
    pub parsed_llm_response: LlmResponse,
    pub final_task_graph: TaskGraph,
    pub execution_success: bool,
    pub task_durations_ms: Vec<(String, u64)>,
    pub execution_errors: Vec<String>,
}

pub struct TraceReplayEngine;

impl TraceReplayEngine {
    /// Save an execution trace to a file on disk.
    pub fn save(trace: &ExecutionTrace, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(trace)
            .map_err(|e| format!("Failed to serialize trace: {}", e))?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, json).map_err(|e| format!("Failed to write trace to file: {}", e))?;
        Ok(())
    }

    /// Load an execution trace from disk.
    pub fn load(path: &Path) -> Result<ExecutionTrace, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read trace file: {}", e))?;
        let trace: ExecutionTrace = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to deserialize trace: {}", e))?;
        Ok(trace)
    }
}
