//! Hierarchical capability graph. Groups capabilities by `group` so the LLM can
//! traverse a tree (`Files → Read/Write/Delete`) instead of reading a flat list —
//! large token savings at M5.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model::Capability;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub name: String,
    pub capability_ids: Vec<String>,
    #[serde(default)]
    pub children: Vec<GraphNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGraph {
    pub roots: Vec<GraphNode>,
}

impl CapabilityGraph {
    /// Build a one-level grouping by `group` (sub-grouping can be added later).
    pub fn from_capabilities(caps: &[Capability]) -> Self {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for c in caps {
            groups
                .entry(c.group.clone())
                .or_default()
                .push(c.id.clone());
        }
        let roots = groups
            .into_iter()
            .map(|(name, capability_ids)| GraphNode {
                name,
                capability_ids,
                children: Vec::new(),
            })
            .collect();
        Self { roots }
    }

    /// JSON form handed to the LLM / cognition subsystem.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}
