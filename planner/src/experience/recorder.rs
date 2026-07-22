//! Workflow Recorder: derives reusable workflows from completed goals.
//!
//! The Recorder is a pure derivation over the Timeline. It watches for
//! `GoalCompleted` events, extracts the semantic action sequence from the
//! goal's timeline entries, and persists it as a [`Workflow`].
//!
//! Per ADR-0008:
//! - The Recorder never mutates execution.
//! - The Recorder never modifies timeline entries.
//! - The Recorder only reads the timeline and writes workflows.

use chrono::Utc;
use uuid::Uuid;

use super::{TimelineEntry, Workflow};
use super::timeline::TimelineStore;

/// Extract a semantic workflow from a completed goal's timeline entries.
///
/// Filters for action/result entries, deduplicates consecutive identical
/// capabilities, and produces a [`Workflow`] ready for persistence.
pub fn extract_workflow(
    store: &TimelineStore,
    goal_id: Uuid,
    goal_name: &str,
) -> anyhow::Result<Option<Workflow>> {
    let entries = store.entries_for_goal(goal_id)?;

    // Need at least one action to form a workflow.
    let actions: Vec<&TimelineEntry> = entries
        .iter()
        .filter(|e| e.event_kind == "exec.action" || e.event_kind == "exec.result")
        .collect();

    if actions.is_empty() {
        return Ok(None);
    }

    // Extract semantic capabilities in order, deduplicating consecutive repeats.
    let mut capabilities = Vec::new();
    let mut last_cap = String::new();
    for entry in &actions {
        if !entry.capability.is_empty() && entry.capability != last_cap {
            last_cap = entry.capability.clone();
            capabilities.push(entry.capability.clone());
        }
    }

    if capabilities.is_empty() {
        return Ok(None);
    }

    // Derive short action names from full capability strings.
    // "mcp:filesystem.write_file" → "filesystem.write_file"
    // "git.commit" → "git.commit"
    let actions: Vec<String> = capabilities
        .iter()
        .map(|c| {
            if let Some(pos) = c.find(':') {
                c[pos + 1..].to_string()
            } else {
                c.clone()
            }
        })
        .collect();

    let workflow = Workflow {
        id: Uuid::new_v4(),
        name: format!("workflow:{}", goal_name),
        actions,
        capabilities,
        created_at: Utc::now(),
        source_goal_id: goal_id,
    };

    Ok(Some(workflow))
}

/// Record a workflow for a completed goal. Combines extraction and persistence.
pub fn record_workflow(
    store: &TimelineStore,
    goal_id: Uuid,
    goal_name: &str,
) -> anyhow::Result<Option<Workflow>> {
    if let Some(workflow) = extract_workflow(store, goal_id, goal_name)? {
        store.save_workflow(&workflow)?;
        Ok(Some(workflow))
    } else {
        Ok(None)
    }
}
