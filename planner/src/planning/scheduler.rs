use uuid::Uuid;

use super::task_graph::TaskGraph;

/// Group a task graph into **levels** for parallel execution. Level 0 contains
/// tasks with no dependencies; each subsequent level contains tasks whose
/// dependencies were all satisfied by earlier levels. Within a level, tasks are
/// independent and may run concurrently. The M3 execution engine processes
/// levels sequentially, but the structure is here so parallelism is a later,
/// isolated change.
pub fn levels(graph: &TaskGraph) -> Result<Vec<Vec<Uuid>>, String> {
    let mut completed: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut result: Vec<Vec<Uuid>> = Vec::new();
    let total = graph.tasks.len();

    while completed.len() < total {
        let ready = graph.ready_tasks(&completed);
        if ready.is_empty() {
            return Err("task graph contains a cycle".to_string());
        }
        for id in &ready {
            completed.insert(*id);
        }
        result.push(ready);
    }
    Ok(result)
}
