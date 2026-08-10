//! FRIDAY Result model — the first-class outcome of a goal execution.
//!
//! A goal's *result* is distinct from its *metadata*. The metadata (phases
//! completed, evidence counts, routing) describes **how** a goal ran; the
//! result is **what** it produced — the actual output of the executed plan,
//! not a reconstruction from bookkeeping. `ExecutiveReport::result` is the
//! canonical home: the Execute phase captures the raw tool output, the report
//! carries it forward, and the portal surfaces it on `goal.completed`.
//!
//! This is the seed of the Result/Artifact model: today the executive produces
//! text/tool results, but the kind taxonomy (`Text`, `Structured`, `File`,
//! `Image`, `Tool`, `Synthesis`, `Error`, `Empty`) is where future artifact
//! kinds land without re-plumbing the pipeline.

use serde::{Deserialize, Serialize};

use crate::ToolResult;

/// The kind of a [`GoalResult`]. Grows with the artifact model — new kinds
/// render differently on the portal without touching the propagation path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GoalResultKind {
    /// A natural-language answer / readable output (e.g. a file listing).
    #[default]
    Text,
    /// A structured payload (JSON) — e.g. a table or an artifact manifest.
    Structured,
    /// A reference to a produced artifact file.
    File,
    /// A produced image.
    Image,
    /// A raw tool result (terminal output, git log, …).
    Tool,
    /// A synthesis across multiple providers/channels.
    Synthesis,
    /// The execution errored; `text` carries the failure message.
    Error,
    /// No result was produced — an explicit empty state, never an invented one.
    Empty,
}

/// The canonical result of one goal execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GoalResult {
    pub kind: GoalResultKind,
    /// Human-readable result text (`""` for `Empty`).
    pub text: String,
    /// Structured payload for `Structured`/`File` kinds (`None` otherwise).
    pub structured: Option<serde_json::Value>,
    /// The tool/skill that produced it, when known.
    pub tool: Option<String>,
    /// Whether the producing execution reported success.
    pub success: bool,
}

impl GoalResult {
    /// The explicit no-result state. Used when a completed goal genuinely
    /// produced nothing — never an invented answer.
    pub fn empty() -> Self {
        Self {
            kind: GoalResultKind::Empty,
            ..Default::default()
        }
    }

    /// Map a raw tool execution to a result: a successful tool with output
    /// becomes a `Text` result (the output is the answer), a successful tool
    /// with no output becomes `Empty`, and a failed tool becomes `Error` with
    /// the failure message. The kind is *derived from the tool outcome*, never
    /// guessed from goal text or phase metadata.
    pub fn from_tool_result(result: &ToolResult) -> Self {
        let kind = if !result.success {
            GoalResultKind::Error
        } else if result.output.trim().is_empty() {
            GoalResultKind::Empty
        } else {
            GoalResultKind::Text
        };
        Self {
            kind,
            text: if result.success {
                result.output.clone()
            } else {
                result.error.clone().unwrap_or_default()
            },
            tool: None,
            success: result.success,
            ..Default::default()
        }
    }

    /// Attach the tool name that produced the result (e.g. `filesystem`).
    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    /// Whether this result carries anything renderable — used by the portal to
    /// fall back to the metadata summary instead of showing a blank card.
    pub fn is_empty(&self) -> bool {
        matches!(self.kind, GoalResultKind::Empty) || self.text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_tool_output_is_a_text_result() {
        let tool = ToolResult::success("Here are the files:\n- Cargo.toml\n- src/");
        let r = GoalResult::from_tool_result(&tool);
        assert_eq!(r.kind, GoalResultKind::Text);
        assert!(r.success);
        assert!(r.text.contains("Here are the files:"));
        assert!(!r.is_empty());
    }

    #[test]
    fn successful_tool_with_no_output_is_empty() {
        let tool = ToolResult::success("");
        let r = GoalResult::from_tool_result(&tool);
        assert_eq!(r.kind, GoalResultKind::Empty);
        assert!(r.is_empty());
        assert!(r.text.is_empty());
    }

    #[test]
    fn failed_tool_is_an_error_result_with_the_message() {
        let tool = ToolResult::error("read_dir failed: no such directory");
        let r = GoalResult::from_tool_result(&tool);
        assert_eq!(r.kind, GoalResultKind::Error);
        assert!(!r.success);
        assert!(r.text.contains("read_dir failed"));
        assert!(!r.is_empty(), "an error message is renderable");
    }

    #[test]
    fn empty_result_is_never_invented() {
        let r = GoalResult::empty();
        assert_eq!(r.kind, GoalResultKind::Empty);
        assert!(r.is_empty());
        assert!(r.text.is_empty());
    }

    #[test]
    fn with_tool_attaches_the_producing_tool() {
        let r = GoalResult::from_tool_result(&ToolResult::success("x")).with_tool("filesystem");
        assert_eq!(r.tool.as_deref(), Some("filesystem"));
    }

    #[test]
    fn result_round_trips_through_json() {
        let r = GoalResult::from_tool_result(&ToolResult::success("answer")).with_tool("git");
        let value = serde_json::to_value(&r).expect("serializes");
        assert_eq!(value["kind"], "text");
        assert_eq!(value["text"], "answer");
        assert_eq!(value["tool"], "git");
        assert_eq!(value["success"], true);
        let back: GoalResult = serde_json::from_value(value).expect("deserializes");
        assert_eq!(back, r);
    }
}
