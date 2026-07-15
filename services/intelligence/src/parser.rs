use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use friday_planner::TaskGraph;

/// The strictly structured JSON format the LLM must respond with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub goal: String,
    pub intent: String,
    pub confidence: f64,
    pub tasks: Vec<ParsedTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTask {
    pub name: String,
    pub capability: String,
    pub params: Value,
    pub depends_on_indices: Vec<usize>,
}

pub struct ResponseInterpreter;

impl ResponseInterpreter {
    /// Parse and validate LLM output. Returns a clean Result containing
    /// the structured `LlmResponse`, or a detailed error string if parsing/ontology checks fail.
    pub fn parse_response(raw: &str, known_capabilities: &[String]) -> Result<LlmResponse, String> {
        // 1. Strip markdown code fences if present
        let mut clean = raw.trim();
        if clean.starts_with("```json") {
            clean = clean.strip_prefix("```json").unwrap_or(clean);
        } else if clean.starts_with("```") {
            clean = clean.strip_prefix("```").unwrap_or(clean);
        }
        if clean.ends_with("```") {
            clean = clean.strip_suffix("```").unwrap_or(clean);
        }
        let clean = clean.trim();

        // 2. Parse JSON
        let parsed: LlmResponse =
            serde_json::from_str(clean).map_err(|e| format!("JSON syntax error: {}", e))?;

        // 3. Ontology Validation (Strict capability check)
        for task in &parsed.tasks {
            let cap = &task.capability;
            if !known_capabilities.contains(cap) {
                // Try fuzzy namespace match, e.g. "windows.launch" -> "windows.open"
                let prefix = cap.split('.').next().unwrap_or(cap);
                let closest = known_capabilities
                    .iter()
                    .filter(|c| c.starts_with(prefix))
                    .min_by_key(|c| {
                        // Very simple distance metric: difference in length
                        (c.len() as i32 - cap.len() as i32).abs()
                    });

                if let Some(suggestion) = closest {
                    return Err(format!(
                        "Ontology Violation: Unknown capability '{}'. Did you mean '{}'?",
                        cap, suggestion
                    ));
                } else {
                    return Err(format!(
                        "Ontology Violation: Capability '{}' does not exist in the registered ontology.",
                        cap
                    ));
                }
            }
        }

        Ok(parsed)
    }

    /// Compile a parsed `LlmResponse` into a structured planner `TaskGraph`.
    pub fn compile_to_task_graph(parsed: &LlmResponse) -> TaskGraph {
        let mut graph = TaskGraph::new(parsed.intent.clone());
        graph.plan_id = Uuid::new_v4();

        let mut added_ids = Vec::new();

        // Pass 1: create all tasks with empty dependencies first to generate Uuids
        for t in &parsed.tasks {
            let id = graph.add_task(t.name.clone(), t.capability.clone(), t.params.clone(), &[]);
            added_ids.push(id);
        }

        // Pass 2: map index-based dependencies onto the task Uuids
        for (i, t) in parsed.tasks.iter().enumerate() {
            let task_id = added_ids[i];
            let mapped_dependencies: Vec<Uuid> = t
                .depends_on_indices
                .iter()
                .filter_map(|&idx| added_ids.get(idx).cloned())
                .collect();

            if let Some(task) = graph.task_mut(task_id) {
                task.depends_on = mapped_dependencies;
            }
        }

        graph
    }
}
