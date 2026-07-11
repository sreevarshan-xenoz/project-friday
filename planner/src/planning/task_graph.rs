use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Lifecycle of a single task. Modelling this explicitly is what makes
/// long-running automations debuggable: at any moment you can ask "what state
/// is task X in?" and get a precise answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Queued,
    Planning,
    Waiting,
    Running,
    Completed,
    Failed,
    Retrying,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Planning => "planning",
            TaskState::Waiting => "waiting",
            TaskState::Running => "running",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Retrying => "retrying",
        }
    }
}

/// One unit of work. `capability` is the event kind an executor plugin
/// subscribes to (e.g. `browser.launch`); `depends_on` lists task ids that
/// must complete before this one may run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub name: String,
    pub capability: String,
    pub params: Value,
    pub depends_on: Vec<Uuid>,
    pub state: TaskState,
}

/// A plan modelled as a **Directed Acyclic Graph**, not a flat list. Independent
/// branches (e.g. open browser + open Spotify) can later run in parallel; the
/// graph makes those dependencies explicit and verifiable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub plan_id: Uuid,
    pub intent: String,
    pub tasks: Vec<Task>,
}

impl TaskGraph {
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            plan_id: Uuid::new_v4(),
            intent: intent.into(),
            tasks: Vec::new(),
        }
    }

    pub fn add_task(
        &mut self,
        name: impl Into<String>,
        capability: impl Into<String>,
        params: Value,
        depends_on: &[Uuid],
    ) -> Uuid {
        let id = Uuid::new_v4();
        self.tasks.push(Task {
            id,
            name: name.into(),
            capability: capability.into(),
            params,
            depends_on: depends_on.to_vec(),
            state: TaskState::Queued,
        });
        id
    }

    pub fn task(&self, id: Uuid) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: Uuid) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Tasks whose dependencies are all in `completed` and which are not yet
    /// completed themselves. These are safe to schedule next.
    pub fn ready_tasks(&self, completed: &HashSet<Uuid>) -> Vec<Uuid> {
        self.tasks
            .iter()
            .filter(|t| !completed.contains(&t.id))
            .filter(|t| t.depends_on.iter().all(|d| completed.contains(d)))
            .map(|t| t.id)
            .collect()
    }

    /// Full topological ordering via Kahn's algorithm. Returns an error if the
    /// graph contains a cycle (which it never should — plans are acyclic).
    pub fn topo_order(&self) -> Result<Vec<Uuid>, String> {
        let mut indeg: HashMap<Uuid, usize> = self
            .tasks
            .iter()
            .map(|t| (t.id, t.depends_on.len()))
            .collect();
        let mut queue: Vec<Uuid> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut order = Vec::new();
        while let Some(id) = queue.pop() {
            order.push(id);
            for t in &self.tasks {
                if t.depends_on.contains(&id) {
                    if let Some(d) = indeg.get_mut(&t.id) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push(t.id);
                        }
                    }
                }
            }
        }
        if order.len() != self.tasks.len() {
            return Err("task graph contains a cycle".to_string());
        }
        Ok(order)
    }
}
