//! `InvestigationChannel` — the information channels a goal can require.

use serde::{Deserialize, Serialize};

/// A distinct kind of information a goal may need before execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InvestigationChannel {
    /// External knowledge: search + fetch web pages.
    Web,
    /// Project/technical documentation.
    Docs,
    /// Understanding of the current repository/codebase.
    CodeIntel,
    /// Long-term memory recall (prior conversation, preferences).
    Memory,
    /// Repository history/working-tree state.
    Git,
    /// Local files and directories.
    Filesystem,
    /// External reasoning through a **logged-in website** (the Browser Organ,
    /// `specs/browser-organ.md`): ask ChatGPT / Grok / Gemini via the browser
    /// — no API keys, the dedicated profile is the credential boundary.
    External,
}

impl InvestigationChannel {
    /// Human label used in the traceable graph.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Docs => "docs",
            Self::CodeIntel => "codeintel",
            Self::Memory => "memory",
            Self::Git => "git",
            Self::Filesystem => "filesystem",
            Self::External => "external",
        }
    }

    /// Capabilities that would satisfy this channel (advisory — the planner
    /// resolves which are registered; e.g. `memory.recall` may not be
    /// registered until the memory API lands).
    pub fn suggested_capabilities(&self) -> &'static [&'static str] {
        match self {
            Self::Web => &["web.search", "web.fetch"],
            Self::Docs => &["docs.search", "docs.read"],
            Self::CodeIntel => &["codeintel.understand"],
            Self::Memory => &["memory.recall"],
            Self::Git => &["git.status", "git.log"],
            Self::Filesystem => &["filesystem.list_directory", "filesystem.read_file"],
            Self::External => &["webagent.ask", "webagent.send"],
        }
    }

    /// Stable ordering used to break confidence ties deterministically.
    pub(crate) fn ordinal(&self) -> usize {
        match self {
            Self::Web => 0,
            Self::Docs => 1,
            Self::CodeIntel => 2,
            Self::Memory => 3,
            Self::Git => 4,
            Self::Filesystem => 5,
            Self::External => 6,
        }
    }

    /// Parse a channel from its wire label (case-insensitive; tolerates `_`
    /// or `-` separators, so `code_intel`, `code-intel` and `file_system` all
    /// resolve). Returns `None` for unknown labels — a model hallucinating a
    /// channel is dropped, never fatal.
    pub fn from_label(s: &str) -> Option<Self> {
        let norm = s.trim().to_ascii_lowercase().replace('_', "-");
        match norm.as_str() {
            "web" => Some(Self::Web),
            "docs" => Some(Self::Docs),
            "codeintel" | "code-intel" => Some(Self::CodeIntel),
            "memory" => Some(Self::Memory),
            "git" => Some(Self::Git),
            "filesystem" | "file-system" => Some(Self::Filesystem),
            "external" | "webagent" | "chatgpt" | "grok" => Some(Self::External),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_suggests_capabilities() {
        for c in [
            InvestigationChannel::Web,
            InvestigationChannel::Docs,
            InvestigationChannel::CodeIntel,
            InvestigationChannel::Memory,
            InvestigationChannel::Git,
            InvestigationChannel::Filesystem,
            InvestigationChannel::External,
        ] {
            assert!(!c.suggested_capabilities().is_empty(), "{}", c.label());
            assert!(!c.label().is_empty());
        }
    }

    #[test]
    fn external_maps_to_webagent_capabilities() {
        assert_eq!(
            InvestigationChannel::External.suggested_capabilities(),
            &["webagent.ask", "webagent.send"]
        );
        assert_eq!(InvestigationChannel::External.label(), "external");
        // The model may say "chatgpt" or "grok" — tolerant label parsing.
        for label in ["external", "webagent", "chatgpt", "grok"] {
            assert_eq!(
                InvestigationChannel::from_label(label),
                Some(InvestigationChannel::External),
                "label: {label}"
            );
        }
    }

    #[test]
    fn git_maps_to_status_and_log() {
        assert_eq!(
            InvestigationChannel::Git.suggested_capabilities(),
            &["git.status", "git.log"]
        );
    }
}
