//! Canonical native capability definitions.
//!
//! These are the *source of truth* for what the built-in service adapters can
//! do, expressed purely as data (`[`model::Capability`]`) with no runtime
//! dependencies. Both the action-runtime adapters (which execute) and the M6
//! planner (which registers per-action bindings into its
//! [`crate::registry::CapabilityRegistry`]) consume this module — so the
//! metadata is defined once.
//!
//! Risk classification lives in the M6 registry (see
//! `planner/src/execution/native_caps.rs`), keyed off the `{adapter}.{action}`
//! ids produced here.

use crate::model::Capability;

/// Native filesystem service capabilities.
pub fn filesystem() -> Vec<Capability> {
    vec![Capability::new("filesystem", "Filesystem Service", "Files")
        .with_actions(&[
            "create_file",
            "read_file",
            "write_file",
            "delete_file",
            "rename",
            "copy",
            "move",
            "list_directory",
        ])
        .with_description("Local filesystem read, write, list and mutation operations")
        .with_latency(5)
        .with_reliability(0.999)
        .with_dry_run(true)]
}

/// Native terminal service capabilities.
pub fn terminal() -> Vec<Capability> {
    vec![Capability::new("terminal", "Terminal Service", "Terminal")
        .with_actions(&["run", "exec", "kill", "list_sessions"])
        .with_description("Persistent shell session command execution")
        .with_latency(50)
        .with_reliability(0.99)
        .with_dry_run(true)]
}

/// Native git service capabilities.
pub fn git() -> Vec<Capability> {
    vec![Capability::new("git", "Git Service", "Git")
        .with_actions(&[
            "clone", "init", "status", "add", "commit", "checkout", "branch", "merge", "diff",
            "push", "pull",
        ])
        .with_description("Version control operations via the Git CLI")
        .with_latency(200)
        .with_reliability(0.97)
        .with_dependencies(&["filesystem"])
        .with_dry_run(true)]
}

/// Native web service capabilities (read-only HTTP: search + fetch/extract).
/// The interactive browser verbs (`open`, `click`, `type`, ...) are *not*
/// native capabilities — they arrive later with a browser runtime and are
/// soft-declared by the bundled `web` skill until then.
pub fn web() -> Vec<Capability> {
    vec![Capability::new("web", "Web Service", "Web")
        .with_actions(&["search", "fetch"])
        .with_description("Read-only web search and page fetch/extract over HTTP")
        .with_latency(800)
        .with_reliability(0.92)
        .with_dry_run(true)]
}

/// Native documentation service capabilities: search, read and parse local docs
/// (READMEs, ADRs, specs, source comments, office files) without a browser.
pub fn docs() -> Vec<Capability> {
    vec![Capability::new("docs", "Documentation Service", "Docs")
        .with_actions(&["search", "read", "parse"])
        .with_description(
            "Search, read and parse local documentation (Markdown, Word, PDF, spreadsheets)",
        )
        .with_latency(20)
        .with_reliability(0.999)
        .with_dry_run(true)]
}

/// Native code-intelligence service capabilities: understand the current
/// repository (structure, file counts, symbol/name matches).
pub fn codeintel() -> Vec<Capability> {
    vec![
        Capability::new("codeintel", "Code Intelligence Service", "Dev")
            .with_actions(&["understand"])
            .with_description("Understand a repository: structure, file counts, and query matches")
            .with_latency(50)
            .with_reliability(0.99)
            .with_dry_run(true),
    ]
}

/// All built-in native service capabilities (filesystem + terminal + git +
/// web + docs + codeintel).
pub fn all() -> Vec<Capability> {
    let mut caps = Vec::new();
    caps.extend(filesystem());
    caps.extend(terminal());
    caps.extend(git());
    caps.extend(web());
    caps.extend(docs());
    caps.extend(codeintel());
    caps
}
