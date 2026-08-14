//! GoalRunner — the daemon's assistant loop (M6.2a wiring, live chat).
//!
//! The portal's Assistant tab sends `planner.goal`; [`crate::CommandGateway`]
//! publishes a `plan.goal` bus event and acks `{accepted: true}`. Until now
//! **nothing consumed that event** — the daemon wired the probe, the bridge,
//! and the gateway, but no planner — so the chat showed the user's message and
//! then silence. [`GoalRunner`] closes the loop:
//!
//! ```text
//! portal: planner.goal
//!     ▼
//! CommandGateway → publish plan.goal
//!     ▼
//! GoalRunner (subscribes plan.goal)
//!     ▼  run the REAL goal-execution Executive with REAL evidence adapters
//!     ▼  (git / web / filesystem / docs / codeintel / webagent / terminal)
//! publish goal.started → goal.completed | goal.failed
//!     ▼
//! BusBridge → portal chat (goal.* kinds render as task cards)
//! ```
//!
//! The Executive is the same runtime the bridge's demo source drives — goals
//! genuinely investigate (evidence gate) → plan → simulate → execute →
//! verify → reflect. The adapters are real (`friday-actions`), so a goal like
//! "check the git status" runs an actual `git status`, and "search the web"
//! performs a real search.
//!
//! Two investigation strategies are available: [`Self::new`] runs the
//! deterministic heuristic (hermetic, tests); [`Self::with_fabric`] wires the
//! real InferenceFabric (the daemon's default) so the **model** decides what
//! evidence a goal needs before planning — with the heuristic as the fallback
//! whenever the model call fails. A failed model is never a failed goal.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use friday_actions::adapters::web::DEFAULT_SEARCH_ENDPOINT;
use friday_actions::{
    ActionAdapter, ActionRequest, CodeIntelAdapter, DocsAdapter, FilesystemAdapter, GitAdapter,
    TerminalAdapter, WebAdapter, WebAgentAdapter,
};
use friday_core::event::{Event, EventBus};
use friday_goal_execution::{
    Action, AdapterTool, Capability, CognitiveBlackboard, CognitiveEpisode, ExecutionContext,
    Executive, ExecutiveReport, GoalResult, MemoryJournal, ResolvingWebTool, Tool, ToolResult,
    WebFetchProvider, WebSearchProvider, WorldState, JINA_READER_PREFIX,
};
use friday_intelligence::InferenceFabric;
use friday_investigation::InvestigationEngine;
use serde_json::json;

/// Real terminal tool for the built-in `ExecuteCommand` capability (the
/// planner's `infer_capability` classifies goals like "run cargo test" as
/// execute-command; the evidence seam only advertises `terminal.run/exec`
/// custom ids, so this tool bridges the built-in capability to the real
/// terminal adapter).
struct TerminalCommandTool {
    adapter: TerminalAdapter,
}

/// Real filesystem tool for the built-in `ListDirectory` / `ReadFile` /
/// `WriteFile` capabilities (the planner's `infer_capability` classifies goals
/// like "list the files in this project" as list-directory; the evidence seam
/// only advertises `filesystem.list_directory` custom ids, so this tool bridges
/// the built-in capabilities to the real filesystem adapter).
struct FilesystemCapabilityTool {
    adapter: FilesystemAdapter,
}

#[async_trait]
impl Tool for TerminalCommandTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn description(&self) -> &str {
        "Runs shell commands (built-in ExecuteCommand capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::ExecuteCommand]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "terminal",
                "run",
                json!({ "command": command_from_instruction(&action.instruction) }),
            ))
            .await;
        ToolResult {
            success: res.success,
            output: serde_json::to_string_pretty(&res.output).unwrap_or_default(),
            error: res.error,
            duration_ms: res
                .metadata
                .get("latency_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            metadata: json!(res.metadata),
        }
    }
}

#[async_trait]
impl Tool for FilesystemCapabilityTool {
    fn name(&self) -> &str {
        "filesystem"
    }

    fn description(&self) -> &str {
        "Lists, reads, writes, trees, and searches files (ListDirectory/ReadFile/WriteFile \
         plus the filesystem.tree/filesystem.search custom ids)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::ListDirectory,
            Capability::ReadFile,
            Capability::WriteFile,
            Capability::Custom("filesystem.tree".into()),
            Capability::Custom("filesystem.search".into()),
        ]
    }

    async fn execute(&self, action: &Action, ctx: &ExecutionContext) -> ToolResult {
        let verb = match &action.capability {
            Capability::ListDirectory => "list_directory",
            Capability::ReadFile => "read_file",
            Capability::WriteFile => "write_file",
            Capability::Custom(id) if id.as_str() == "filesystem.tree" => "tree",
            Capability::Custom(id) if id.as_str() == "filesystem.search" => "search",
            _ => {
                return ToolResult::error(
                    "filesystem tool only handles the ListDirectory/ReadFile/WriteFile \
                     capabilities and the filesystem.tree/filesystem.search custom ids",
                )
            }
        };
        let mut params = action.parameters.clone();
        let cwd = ctx
            .working_directory
            .clone()
            .unwrap_or_else(|| ".".to_string());
        // A direct goal's action carries no path — default to the context's
        // working directory (or the daemon's cwd) so "list the files in this
        // project" resolves to a real listing instead of a missing-parameter
        // error. Reads name a specific file, so resolve it ("Read the
        // contents of planning.rs" must read planning.rs, not the directory).
        if params.get("path").is_none() {
            if verb == "read_file" {
                match read_path_from_instruction(&self.adapter, &action.instruction, &cwd).await {
                    Ok(p) => params["path"] = json!(p),
                    Err(e) => return ToolResult::error(e),
                }
            } else {
                params["path"] = json!(cwd);
            }
        }
        if verb == "search" {
            if let Some(extra) = search_params(&action.instruction) {
                if let Some(map) = params.as_object_mut() {
                    for (key, value) in extra {
                        map.insert(key, value);
                    }
                }
            }
        }
        let res = self
            .adapter
            .execute(ActionRequest::new("filesystem", verb, params))
            .await;
        ToolResult {
            success: res.success,
            output: serde_json::to_string_pretty(&res.output).unwrap_or_default(),
            error: res.error,
            duration_ms: res
                .metadata
                .get("latency_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            metadata: json!(res.metadata),
        }
    }
}

/// Real web-search tool for the built-in `SearchWeb` capability (the planner's
/// `infer_capability` classifies "search the web for X" as search-web). The
/// evidence seam only advertises `web.search`, so this tool bridges the
/// built-in capability to the real read-only [`WebAdapter`].
struct WebSearchCapabilityTool {
    adapter: WebAdapter,
}

#[async_trait]
impl Tool for WebSearchCapabilityTool {
    fn name(&self) -> &str {
        "web"
    }

    fn description(&self) -> &str {
        "Searches the web and returns structured results (built-in SearchWeb capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::SearchWeb]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "web",
                "search",
                json!({ "query": web_query_from_instruction(&action.instruction) }),
            ))
            .await;
        ToolResult {
            success: res.success,
            output: serde_json::to_string_pretty(&res.output).unwrap_or_default(),
            error: res.error,
            duration_ms: res
                .metadata
                .get("latency_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            metadata: json!(res.metadata),
        }
    }
}

/// Real memory tool for the built-in `SearchMemory` capability (the planner's
/// `infer_capability` classifies "recall …" and unclassified statements as
/// memory recall). Reads the *shared durable episode journal* — the same store
/// the Executive writes through its admission gate — so recall answers from
/// FRIDAY's persistent memory (S2), never from a second, parallel store.
struct MemoryRecallTool {
    journal: Arc<MemoryJournal>,
}

/// Real memory tool for the `memory.remember` custom capability (the planner
/// routes "remember X" here). The acknowledgment IS the execution: the goal's
/// episode — whose description carries the fact — passes the admission gate and
/// is stored + journaled by the Executive, so the fact enters the same
/// persistent memory the recall tool reads.
struct MemoryRememberTool {}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Recalls what FRIDAY knows: searches persistent episodic memory for entries relevant to the query (SearchMemory capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::SearchMemory,
            Capability::Custom("memory.recall".into()),
        ]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let hits = self
            .journal
            .recall(&action.instruction, MEMORY_RECALL_LIMIT);
        if hits.is_empty() {
            return ToolResult::success("no memory matches yet");
        }
        let noun = if hits.len() == 1 { "entry" } else { "entries" };
        let body: Vec<String> = hits
            .iter()
            .enumerate()
            .map(|(i, ep)| {
                let result = ep
                    .execution_result
                    .as_ref()
                    .map(|r| r.output.trim())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("<no result>");
                format!(
                    "{}. {} — {}",
                    i + 1,
                    ep.goal.description,
                    truncate(result, MAX_CONTEXT_CHARS)
                )
            })
            .collect();
        ToolResult::success(format!(
            "recalled {} memory {noun}:\n{}",
            hits.len(),
            body.join("\n")
        ))
    }
}

#[async_trait]
impl Tool for MemoryRememberTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Writes a fact to memory: acknowledges the instruction so its episode (goal + result) is stored in persistent episodic memory (memory.remember)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom("memory.remember".into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        ToolResult::success(format!("remembered: {}", action.instruction))
    }
}

/// The query a web-search goal names: the instruction minus the routing verbs
/// that got us here ("search the web for …"). Kept minimal — the remainder is
/// the search text, so "Search the web for the latest Rust async docs" queries
/// `latest Rust async docs` rather than the whole sentence.
fn web_query_from_instruction(instruction: &str) -> String {
    let mut q = instruction.trim().to_string();
    let lower = q.to_lowercase();
    for prefix in [
        "search the web for ",
        "search the web ",
        "search online for ",
        "search for ",
        "search ",
        "look up ",
        "find ",
        "research ",
    ] {
        if lower.starts_with(prefix) {
            q = q[prefix.len()..].trim_start().to_string();
            break;
        }
    }
    let lower = q.to_lowercase();
    for prefix in ["the ", "latest "] {
        if lower.starts_with(prefix) {
            q = q[prefix.len()..].trim_start().to_string();
        }
    }
    if q.is_empty() {
        instruction.to_string()
    } else {
        q
    }
}
fn command_from_instruction(instruction: &str) -> String {
    let trimmed = instruction.trim();
    for prefix in ["run ", "Run ", "RUN ", "execute ", "Execute ", "EXECUTE "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    instruction.to_string()
}

/// The bare filename an instruction names, e.g. `planning.rs` from "Read the
/// contents of planning.rs". Requires a `.` extension; relative paths like
/// `src/main.rs` are kept whole so they can be joined to the working dir.
fn filename_token(instruction: &str) -> Option<String> {
    for tok in instruction.split_whitespace() {
        let t = tok.trim_matches(|c: char| c.is_ascii_punctuation());
        if !t.contains('.') {
            continue;
        }
        let ext = t.rsplit('.').next().unwrap_or("");
        if (1..=6).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(t.to_string());
        }
    }
    None
}

/// Resolve the path a read goal names: the file as given relative to the
/// working directory, else the first filesystem.search hit by name, else an
/// honest error instead of reading a directory.
async fn read_path_from_instruction(
    adapter: &FilesystemAdapter,
    instruction: &str,
    cwd: &str,
) -> Result<String, String> {
    let Some(name) = filename_token(instruction) else {
        return Ok(cwd.to_string());
    };
    // Try the name as given relative to the working dir ("read src/main.rs"),
    // then fall back to a name search for the bare file ("Read the contents
    // of planning.rs").
    let joined = std::path::Path::new(cwd).join(&name);
    if joined.is_file() {
        return Ok(joined.to_string_lossy().to_string());
    }
    let base = std::path::Path::new(&name)
        .file_name()
        .and_then(|b| b.to_str())
        .unwrap_or(&name);
    let res = adapter
        .execute(ActionRequest::new(
            "filesystem",
            "search",
            json!({ "path": cwd, "query": base }),
        ))
        .await;
    if res.success {
        if let Some(matches) = res.output.get("matches").and_then(|m| m.as_array()) {
            for m in matches {
                let is_dir = m.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
                if let (Some(p), false) = (m.get("path").and_then(|v| v.as_str()), is_dir) {
                    return Ok(p.to_string());
                }
            }
        }
    }
    Err(format!("file '{name}' not found"))
}

/// Derive search parameters from a direct goal's instruction.
///
/// Two shapes are recognised:
/// - name search: "Find all Rust files" → query "rust" + ext "rs", so the
///   recursive search actually finds `.rs` files instead of matching the whole
///   sentence verbatim;
/// - content search: "Find the Rust files containing ExpertRouter" → content
///   "ExpertRouter" + ext "rs" — the "containing" phrasing means match file
///   *contents*, not names, so symbols/prose can be found.
fn search_params(instruction: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let lower = instruction.to_lowercase();
    let stopwords = [
        "find",
        "search",
        "list",
        "show",
        "give",
        "all",
        "any",
        "the",
        "a",
        "an",
        "for",
        "of",
        "in",
        "on",
        "within",
        "files",
        "file",
        "named",
        "called",
        "folder",
        "folders",
        "directory",
        "directories",
        "project",
        "workspace",
        "and",
        "or",
        "that",
        "with",
        "me",
        "up",
        "out",
    ];
    // Content-intent words: "files containing X" searches contents, not names.
    let content_intent = [
        "containing",
        "contains",
        "contain",
        "content",
        "mention",
        "mentions",
        "mentioning",
        "include",
        "includes",
        "including",
        "reference",
        "references",
        "referencing",
        "uses",
        "using",
        "use",
    ];
    let language_words = [
        ("rust", "rs"),
        ("python", "py"),
        ("javascript", "js"),
        ("typescript", "ts"),
        ("toml", "toml"),
        ("markdown", "md"),
        ("json", "json"),
    ];
    let wants_content = content_intent
        .iter()
        .any(|w| lower.split_whitespace().any(|t| t == *w));
    let terms: Vec<String> = lower
        .split_whitespace()
        .filter(|t| !stopwords.contains(t) && !content_intent.contains(t))
        .map(str::to_string)
        .collect();
    let lang = language_words
        .iter()
        .find(|(w, _)| terms.iter().any(|t| t == w));
    let lang_word = lang.map(|(w, _)| *w);
    let lang_ext = lang.map(|(_, ext)| *ext);
    let mut content_terms: Vec<String> = terms
        .iter()
        .filter(|t| language_words.iter().all(|(w, _)| t != w))
        .cloned()
        .collect();

    let mut params = serde_json::Map::new();
    if wants_content {
        // A content query needs a substantive needle; if only the language word
        // survived (e.g. "files containing rust") the language word *is* the
        // needle and there is no extension filter.
        if content_terms.is_empty() {
            if let Some(w) = lang_word {
                content_terms.push(w.to_string());
            }
        } else if let Some(e) = lang_ext {
            params.insert("ext".into(), json!(e));
        }
        if !content_terms.is_empty() {
            params.insert("content".into(), json!(content_terms.join(" ")));
        }
    } else {
        if !terms.is_empty() {
            params.insert("query".into(), json!(terms.join(" ")));
        }
        if let Some(e) = lang_ext {
            params.insert("ext".into(), json!(e));
        }
    }
    (!params.is_empty()).then_some(params)
}

/// Session identity for `plan.goal` events published without one (older portal
/// builds): the goal runs under this fixed session so continuity still applies
/// across its goals instead of degrading to an empty conversation.
const DEFAULT_SESSION_ID: &str = "default";

/// Hard cap on tracked sessions. Least-recently-used entries are evicted so a
/// flood of short-lived sessions cannot grow the map unboundedly.
const MAX_SESSIONS: usize = 32;

/// Per-context character cap (applied to prior goal/result text) so a
/// long-running session never balloons the composed goal handed to the
/// Executive.
const MAX_CONTEXT_CHARS: usize = 400;

/// How many memory entries the Context Builder recalls into the composed goal
/// and the recall tool returns — bounded so memory never floods a turn.
const MEMORY_RECALL_LIMIT: usize = 3;

/// One session's conversational state — held in memory only (S1; persistence
/// is the separate memory-write slice). `last_goal` / `last_result` describe
/// the just-finished turn and are composed into the *next* goal the Executive
/// sees, so the planner and its investigation are grounded in the prior turn
/// instead of starting from an empty conversation.
#[derive(Debug, Clone)]
struct SessionContext {
    turn: u64,
    last_goal: String,
    last_result: String,
    last_seen_ms: u64,
}

impl SessionContext {
    fn new() -> Self {
        Self {
            turn: 0,
            last_goal: String::new(),
            last_result: String::new(),
            last_seen_ms: now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Compose the goal text the Executive plans on: the user's goal first, then —
/// when the session has a prior turn — a clearly-delimited, size-capped
/// `[session context]` block carrying the previous goal and result, and when
/// memory recall returned relevant entries — a `[memory]` block carrying what
/// FRIDAY remembers. This rides the existing context architecture (the goal
/// description feeds both the intent classifier and the LLM investigation
/// prompt), so a later goal resolves references to prior turns *and* to facts
/// from earlier sessions. The blocks are appended *after* the current goal so
/// the current intent reads first; the heuristic classifier's whole-word
/// keyword scan can still be influenced by echoed words — accepted for the
/// heuristic fallback, irrelevant to the LLM-backed path that powers the
/// daemon.
fn compose_goal_with_context(goal: &str, prior: &SessionContext, recalled: &[String]) -> String {
    let mut out = String::with_capacity(goal.len() + 640);
    out.push_str(goal);
    if prior.turn > 0 {
        out.push_str("\n\n[session context]\n");
        out.push_str("previous goal: ");
        out.push_str(&truncate(&prior.last_goal, MAX_CONTEXT_CHARS));
        if !prior.last_result.is_empty() {
            out.push_str("\nprevious result: ");
            out.push_str(&truncate(&prior.last_result, MAX_CONTEXT_CHARS));
        }
    }
    if !recalled.is_empty() {
        out.push_str("\n\n[memory]\n");
        for line in recalled {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Format one recalled episode as a `[memory]` block line: the episode's goal
/// and its result, both size-capped.
fn format_recalled_memory(ep: &CognitiveEpisode) -> String {
    let result = ep
        .execution_result
        .as_ref()
        .map(|r| r.output.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("<no result>");
    format!(
        "goal: {}\nresult: {}",
        truncate(&ep.goal.description, MAX_CONTEXT_CHARS),
        truncate(result, MAX_CONTEXT_CHARS)
    )
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Keep the session map bounded: drop the least-recently-seen entries until
/// the map is at most `max` sessions.
fn evict_lru(sessions: &Mutex<HashMap<String, SessionContext>>, max: usize) {
    let mut sessions = sessions.lock().unwrap();
    while sessions.len() > max {
        let oldest = sessions
            .iter()
            .min_by_key(|(_, ctx)| ctx.last_seen_ms)
            .map(|(k, _)| k.clone());
        match oldest {
            Some(k) => {
                sessions.remove(&k);
            }
            None => break,
        }
    }
}

/// The daemon's assistant loop: consumes `plan.goal` bus events and runs each
/// goal through the real goal-execution `Executive`, publishing
/// `goal.started` / `goal.completed` / `goal.failed` events back to the bus so
/// the portal chat shows real FRIDAY activity instead of a silent ack.
pub struct GoalRunner {
    bus: EventBus,
    executive: tokio::sync::Mutex<Executive>,
    workspace: Arc<CognitiveBlackboard>,
    /// The fabric backing the LLM investigation. Retained (one Arc clone) so
    /// the runner can read the routing decision after each cycle and surface
    /// on `goal.completed` which provider/model actually answered. `None` for
    /// the heuristic-only runner (`Self::new`).
    fabric: Option<Arc<InferenceFabric>>,
    /// Per-session conversational context (`session_id` → context), bounded by
    /// [`MAX_SESSIONS`] with LRU eviction. Locked only in short critical
    /// sections — never held across an await.
    sessions: Mutex<HashMap<String, SessionContext>>,
    /// The durable memory journal this runner writes through (shared with the
    /// Executive's `EpisodeStore` and the recall tool — one store, one
    /// journal). `None` = memory stays in-memory only (heuristic test runners
    /// and any caller that does not opt into persistence).
    journal: Option<Arc<MemoryJournal>>,
}

impl GoalRunner {
    /// The assistant loop with the deterministic heuristic investigation
    /// strategy — hermetic (no model calls), used by tests.
    pub fn new(bus: EventBus) -> Self {
        Self::with_investigation_engine(bus, InvestigationEngine::heuristic(), None, None, None)
    }

    /// The daemon's LLM-backed assistant loop: the InferenceFabric derives the
    /// evidence requirements (real model judgment gates planning), and the
    /// heuristic catches any model failure — an unreachable provider never
    /// stalls or fails a goal (`LlmInvestigationStrategy` guarantees the
    /// fallback, observably via `[Investigation Strategy]` trace lines).
    pub fn with_fabric(bus: EventBus, fabric: Arc<InferenceFabric>) -> Self {
        // `Arc<T>: LlmStrategyClient` (the blanket impl) lets the strategy own
        // the shared fabric without cloning or unwrapping — the natural way a
        // daemon hands its fabric to the executive. One clone is retained here
        // so `run_goal` can read the fabric's routing decision afterwards.
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::llm(Box::new(fabric.clone())),
            Some(fabric),
            None,
            None,
        )
    }

    /// Hermetic runner for tests: the web-search tool points at a local mock
    /// endpoint instead of the real search provider, so an end-to-end web goal
    /// test needs no network.
    pub fn with_web_endpoint(bus: EventBus, endpoint: impl Into<String>) -> Self {
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::heuristic(),
            None,
            Some(endpoint.into()),
            None,
        )
    }

    /// Hermetic runner with persistent memory: the same heuristic strategy as
    /// [`Self::new`], but every admitted episode is written to the shared
    /// `journal` and recalled into later goals — so a test can restart the
    /// runner on the same journal and prove memory survives the process.
    pub fn with_memory(bus: EventBus, journal: Arc<MemoryJournal>) -> Self {
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::heuristic(),
            None,
            None,
            Some(journal),
        )
    }

    /// The daemon's LLM-backed assistant loop with persistent memory: the
    /// fabric powers investigation and the shared `journal` makes memory survive
    /// restarts (close FRIDAY, reopen it tomorrow, it still knows).
    pub fn with_fabric_and_memory(
        bus: EventBus,
        fabric: Arc<InferenceFabric>,
        journal: Arc<MemoryJournal>,
    ) -> Self {
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::llm(Box::new(fabric.clone())),
            Some(fabric),
            None,
            Some(journal),
        )
    }

    /// Shared constructor: the Executive plus every real evidence adapter and
    /// the advertised world-state surface, with the chosen investigation
    /// strategy injected (heuristic or fabric-backed) and the fabric the
    /// runner should query for the served route decision (`None` = heuristic).
    /// `web_endpoint` overrides the web-search tool's endpoint for tests.
    /// `journal` attaches persistent memory: the Executive seeds its episode
    /// store from it, every admitted episode is appended to it, and the memory
    /// recall/remember tools read/write it (`None` = in-memory only).
    fn with_investigation_engine(
        bus: EventBus,
        investigation: InvestigationEngine,
        fabric: Option<Arc<InferenceFabric>>,
        web_endpoint: Option<String>,
        journal: Option<Arc<MemoryJournal>>,
    ) -> Self {
        let workspace = CognitiveBlackboard::new();
        let mut executive = Executive::new().with_investigation_engine(investigation);
        if let Some(journal) = &journal {
            // One journal, one memory: seed the Executive's episode store from
            // it and give the built-in SearchMemory capability a real tool that
            // reads the same journal, so memory survives restarts.
            executive = executive.with_episode_journal(journal.clone());
            executive.register_tool(Box::new(MemoryRecallTool {
                journal: journal.clone(),
            }));
            executive.register_tool(Box::new(MemoryRememberTool {}));
        }

        // The dedicated filesystem tool first: the planner's direct goals carry
        // the *built-in* capabilities plus the `filesystem.tree`/`filesystem.
        // search` custom ids, and the registry resolves first-match. Registered
        // ahead of `evidence:filesystem` (which also advertises those ids via
        // the adapter's verbs) so a plain "show the tree / find a file" goal is
        // served by the dedicated tool, not hijacked by the evidence wrapper —
        // whose `execute` would refuse the non-evidence instruction. Evidence
        // actions (`gather evidence: filesystem (filesystem.list_directory)`)
        // still resolve to `evidence:filesystem`: the investigation channels
        // only ever suggest list_directory/read_file, ids this tool does not
        // claim.
        executive.register_tool(Box::new(FilesystemCapabilityTool {
            adapter: FilesystemAdapter::new(),
        }));
        // The built-in SearchWeb capability → the real read-only web adapter.
        // Registered with the other dedicated tools (ahead of `evidence:web`)
        // so a direct "search the web" goal resolves here, not to the evidence
        // wrapper. The same configured adapter backs the evidence seam too, so
        // a test-pointed endpoint keeps every web action hermetic.
        let web_adapter = match &web_endpoint {
            Some(endpoint) => WebAdapter::with_endpoint(endpoint.clone()),
            None => WebAdapter::new(),
        };
        executive.register_tool(Box::new(WebSearchCapabilityTool {
            adapter: web_adapter.clone(),
        }));
        // web EVIDENCE actions (web.search + web.fetch) route through the
        // binding resolver (health-gated provider fallback) instead of a
        // single hardwired adapter — registered ahead of the plain evidence
        // adapters so both verbs resolve here. Search: the configured endpoint
        // (or the default) as the `duckduckgo` provider. Fetch: the `jina`
        // gateway (Jina Reader, `https://r.jina.ai/` — the target URL is
        // percent-encoded into the gateway URL by the dispatch) first, with
        // the `direct` plain-HTTP-GET provider as the health-gated fallback
        // when the gateway is unreachable. The same configured endpoint keeps
        // the hermetic test path intact.
        let search_endpoint = web_endpoint
            .clone()
            .unwrap_or_else(|| DEFAULT_SEARCH_ENDPOINT.to_string());
        executive.register_tool(Box::new(ResolvingWebTool::for_web(
            Arc::new(web_adapter.clone()),
            vec![WebSearchProvider::new("duckduckgo", search_endpoint)],
            vec![
                WebFetchProvider::gateway("jina", JINA_READER_PREFIX),
                WebFetchProvider::new("direct", ""),
            ],
        )));
        // Real evidence adapters — the same seam the bridge/daemon use
        // (`AdapterTool::for_evidence`), so evidence actions run real tools.
        // The web adapter is deliberately NOT in this list: the resolver-backed
        // tool above is now the web evidence tool for both verbs.
        let evidence_adapters: Vec<Arc<dyn ActionAdapter>> = vec![
            Arc::new(GitAdapter::new()),
            Arc::new(FilesystemAdapter::new()),
            Arc::new(DocsAdapter::new()),
            Arc::new(CodeIntelAdapter::new()),
            Arc::new(WebAgentAdapter::new()),
        ];
        for adapter in evidence_adapters {
            executive.register_tool(Box::new(AdapterTool::for_evidence(adapter)));
        }
        // The built-in ExecuteCommand capability (planner's infer_capability
        // default) → the real terminal adapter.
        executive.register_tool(Box::new(TerminalCommandTool {
            adapter: TerminalAdapter::new(),
        }));

        // Advertise the same surface to the evaluator/simulator so evidence
        // plans are judged feasible and actually execute.
        let mut world_state = WorldState::new()
            .with_tool(Capability::SearchWeb)
            .with_tool(Capability::ReadFile)
            .with_tool(Capability::WriteFile)
            .with_tool(Capability::ListDirectory)
            .with_tool(Capability::ExecuteCommand)
            .with_tool(Capability::Custom("web.search".into()))
            .with_tool(Capability::Custom("web.fetch".into()))
            .with_tool(Capability::Custom("git.status".into()))
            .with_tool(Capability::Custom("git.log".into()))
            .with_tool(Capability::Custom("docs.search".into()))
            .with_tool(Capability::Custom("docs.read".into()))
            .with_tool(Capability::Custom("codeintel.understand".into()))
            .with_tool(Capability::Custom("filesystem.list_directory".into()))
            .with_tool(Capability::Custom("filesystem.read_file".into()))
            .with_tool(Capability::Custom("filesystem.tree".into()))
            .with_tool(Capability::Custom("filesystem.search".into()))
            .with_tool(Capability::SearchMemory)
            .with_tool(Capability::Custom("webagent.ask".into()))
            .with_tool(Capability::Custom("webagent.send".into()))
            .with_tool(Capability::Custom("webagent.read".into()));
        if journal.is_some() {
            world_state = world_state.with_tool(Capability::Custom("memory.remember".into()));
        }
        workspace.set_world_state(world_state);

        Self {
            bus,
            executive: tokio::sync::Mutex::new(executive),
            workspace,
            fabric,
            sessions: Mutex::new(HashMap::new()),
            journal,
        }
    }

    /// Subscribe to `plan.goal` and run each goal through the Executive,
    /// publishing lifecycle events so the portal chat responds.
    ///
    /// The subscription is created *synchronously* before the task is spawned
    /// so a `plan.goal` published immediately after `start()` returns is never
    /// missed (broadcast receivers must exist before the send).
    pub fn start(self) {
        let mut rx = self.bus.subscribe();
        tokio::spawn(async move {
            tracing::info!("goal runner subscribed to plan.goal");
            while let Ok(ev) = rx.recv().await {
                if ev.kind != "plan.goal" {
                    continue;
                }
                let goal = ev
                    .payload
                    .get("goal")
                    .and_then(|g| g.as_str())
                    .unwrap_or("")
                    .to_string();
                if goal.trim().is_empty() {
                    continue;
                }
                let session_id = ev
                    .payload
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(DEFAULT_SESSION_ID)
                    .to_string();
                self.run_goal(goal, session_id).await;
            }
        });
    }

    /// Run one goal through the Executive and publish its lifecycle events,
    /// keyed by the session the goal arrived from. The session's prior turn is
    /// composed into the goal text the Executive plans on (per-session
    /// continuity), and the finished turn is recorded for the next goal in the
    /// same session.
    async fn run_goal(&self, goal: String, session_id: String) {
        let id = format!("goal-{}", uuid::Uuid::new_v4().simple());

        // Read the session's prior turn (short lock) and reserve the next turn
        // number. `prior` is snapshotted BEFORE this turn is written back, so
        // goal N+1 is composed from turn N's goal/result.
        let (prior, next_turn) = {
            let mut sessions = self.sessions.lock().unwrap();
            let ctx = sessions
                .entry(session_id.clone())
                .or_insert_with(SessionContext::new);
            ctx.last_seen_ms = now_ms();
            let prior = ctx.clone();
            ctx.turn += 1;
            (prior, ctx.turn)
        };

        // Recall the relevant entries into the Context Builder *before*
        // composing, so `[memory]` joins `[session context]` in the single
        // context assembly path the Executive plans on. Recall runs on the RAW
        // goal (the user's own words), not the composed text. The durable
        // journal is the source of truth when one is attached; otherwise the
        // in-memory episode store backs the same recall.
        let recalled: Vec<String> = {
            let exec = self.executive.lock().await;
            let hits: Vec<CognitiveEpisode> = match &self.journal {
                Some(journal) => journal.recall(&goal, MEMORY_RECALL_LIMIT),
                None => exec
                    .episode_store()
                    .recall(&goal, MEMORY_RECALL_LIMIT)
                    .into_iter()
                    .cloned()
                    .collect(),
            };
            hits.iter().map(format_recalled_memory).collect()
        };
        let composed = compose_goal_with_context(&goal, &prior, &recalled);

        let _ = self
            .bus
            .publish(Event::new(
                "goal.started",
                "planner",
                json!({
                    "goal": goal,
                    "id": id,
                    "session_id": session_id,
                    "session_turn": next_turn,
                }),
            ))
            .await;

        let report = {
            let mut exec = self.executive.lock().await;
            // Keep focus deterministic: drop stale attention items so the fed
            // goal is always the next focus target.
            exec.attention_manager_mut().clear();
            exec.add_goal(composed, 0.9);
            exec.cycle(&self.workspace).await
        };

        // Record the finished turn for the next goal in this session.
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(ctx) = sessions.get_mut(&session_id) {
                ctx.last_goal = goal.clone();
                ctx.last_result = report
                    .result
                    .as_ref()
                    .map(|r| truncate(&r.text, MAX_CONTEXT_CHARS))
                    .unwrap_or_default();
                ctx.last_seen_ms = now_ms();
            }
        }
        evict_lru(&self.sessions, MAX_SESSIONS);

        if report.plan_executed {
            // The fabric is called exactly once per goal (the investigation), so
            // its last routing decision is THIS goal's: `served` is
            // Some((provider, model)) when the model answered, None when it
            // errored and the heuristic planned instead.
            let served = self
                .fabric
                .as_ref()
                .and_then(|f| f.last_decision())
                .and_then(|d| d.served);
            let mut payload = completed_payload(&goal, &id, &report, served.as_ref());
            payload["session_id"] = json!(session_id);
            payload["session_turn"] = json!(next_turn);
            let _ = self
                .bus
                .publish(Event::new("goal.completed", "planner", payload))
                .await;
        } else {
            let reason = report
                .error
                .unwrap_or_else(|| "no plan executed — goal may be infeasible".to_string());
            let _ = self
                .bus
                .publish(Event::new(
                    "goal.failed",
                    "planner",
                    json!({
                        "goal": goal,
                        "id": id,
                        "session_id": session_id,
                        "session_turn": next_turn,
                        "error": reason,
                    }),
                ))
                .await;
        }
    }
}

/// Build the `goal.completed` payload: the goal's own facts, the evidence
/// channels (one `{channel, verified}` per evidence action), the goal's
/// canonical **result** (the actual execution output — never a reconstruction
/// from metadata; an explicit empty result when nothing was produced), and —
/// when the fabric served a model — the route decision `(provider, model)`
/// plus the investigation strategy that produced the plan (`llm` = the model
/// answered, `heuristic` = the fallback planned). The portal chat renders
/// these as the task-card detail so the user sees *what evidence was gathered,
/// which model answered, and what FRIDAY actually produced*.
fn completed_payload(
    goal: &str,
    id: &str,
    report: &ExecutiveReport,
    served: Option<&(String, String)>,
) -> serde_json::Value {
    let channels: Vec<serde_json::Value> = report
        .evidence_trail
        .records
        .iter()
        .map(|r| json!({ "channel": r.channel, "verified": r.verified }))
        .collect();
    // A completed goal always carries a well-formed result: the executive's
    // captured output, or an explicit Empty state when it produced nothing.
    let result = report.result.clone().unwrap_or_else(GoalResult::empty);
    let mut payload = json!({
        "goal": goal,
        "id": id,
        "phases": report.phases_completed.len(),
        "evidence_verified": report.evidence_trail.verified_count(),
        "strategy": if served.is_some() { "llm" } else { "heuristic" },
        "channels": channels,
        "result": result,
    });
    if let Some((provider, model)) = served {
        payload["provider"] = json!(provider);
        payload["model"] = json!(model);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn goal_runner_publishes_lifecycle_events_for_a_goal() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        // The CommandGateway publishes exactly this when `planner.goal`
        // arrives from the portal.
        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Check the git status of the workspace repository" }),
        ))
        .await
        .expect("publish");

        // Expect goal.started then a terminal event (completed or failed) —
        // the chat must never be left hanging after the ack.
        let mut saw_started = false;
        let mut saw_terminal = false;
        for _ in 0..20 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.started" => {
                    saw_started = true;
                    assert_eq!(
                        ev.payload["goal"],
                        "Check the git status of the workspace repository"
                    );
                }
                "goal.completed" | "goal.failed" => {
                    saw_terminal = true;
                    assert_eq!(ev.payload["id"], ev.payload["id"]);
                    if ev.kind == "goal.completed" {
                        // The completed card detail: the evidence channels are
                        // attached (git fired and verified for this goal), the
                        // heuristic runner reports no served model, and the
                        // goal carries a well-formed result (the executive's
                        // captured output, or an explicit empty state).
                        let channels = ev.payload["channels"].as_array().expect("channels");
                        assert!(
                            channels
                                .iter()
                                .any(|c| { c["channel"] == "git" && c["verified"] == json!(true) }),
                            "git evidence verified: {channels:?}"
                        );
                        assert_eq!(ev.payload["strategy"], "heuristic");
                        assert!(
                            ev.payload.get("provider").is_none(),
                            "no model served on the heuristic runner"
                        );
                        let result = ev.payload["result"].as_object().expect("result object");
                        assert!(
                            result["kind"].is_string(),
                            "result carries a kind: {result:?}"
                        );
                        assert!(
                            result["text"].is_string(),
                            "result carries renderable text (may be empty): {result:?}"
                        );
                    }
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_started, "goal.started must be published");
        assert!(
            saw_terminal,
            "a goal must always reach a terminal event (completed or failed)"
        );
    }

    /// The daemon's LLM-backed path must degrade gracefully: an InferenceFabric
    /// with no providers cannot route (fast error, `LlmInvestigationStrategy`
    /// falls back to the heuristic), so the goal still reaches a terminal
    /// event — a failed model call is never a failed goal.
    #[tokio::test]
    async fn goal_runner_with_empty_fabric_falls_back_to_heuristic() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_fabric(bus.clone(), Arc::new(InferenceFabric::new()));
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Check the git status of the workspace repository" }),
        ))
        .await
        .expect("publish");

        let mut saw_started = false;
        let mut saw_terminal = false;
        for _ in 0..20 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.started" => saw_started = true,
                "goal.completed" | "goal.failed" => {
                    saw_terminal = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_started, "goal.started must be published");
        assert!(
            saw_terminal,
            "an empty fabric must still reach a terminal event (heuristic fallback), not stall"
        );
    }

    /// The SearchWeb milestone: "Search the web for the latest Rust async docs"
    /// must route through the real web-search tool and reach `goal.completed`
    /// with **structured** results. The mock search endpoint keeps the test
    /// hermetic — no network — and records the request line so the test proves
    /// the real query (not the whole sentence) was sent.
    #[tokio::test]
    async fn web_goal_reaches_goal_completed_with_structured_results() {
        use std::io::{Read, Write};

        // A tiny canned search-results page in the endpoint's HTML shape.
        let body = r#"<html><body>
            <a href="https://docs.rs/tokio/latest/tokio/">Tokio documentation</a>
            <a href="https://rust-lang.github.io/async-book/">The Rust Async Book</a>
        </body></html>"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking mock server");
        let addr = listener.local_addr().expect("mock server address");
        let requests = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let requests_clone = requests.clone();
        let server = std::thread::spawn(move || {
            // Serve every request that arrives (the evidence web.search fires
            // before the goal's own SearchWeb, so the endpoint sees ≥2 hits);
            // exit once served (or after ~15s so a broken run never hangs).
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut served = 0usize;
            let mut last_activity = std::time::Instant::now();
            while std::time::Instant::now() < deadline && served < 5 {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // Idle for 3s → the goal finished its requests.
                        if last_activity.elapsed() > std::time::Duration::from_secs(3) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => panic!("mock server accept: {e}"),
                };
                last_activity = std::time::Instant::now();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let line = String::from_utf8_lossy(&buf)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                requests_clone.lock().unwrap().push(line);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                served += 1;
            }
        });

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_web_endpoint(bus.clone(), format!("http://{addr}/"));
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Search the web for the latest Rust async docs" }),
        ))
        .await
        .expect("publish");

        let mut saw_started = false;
        let mut terminal = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.started" => saw_started = true,
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev);
                    break;
                }
                _ => {}
            }
        }
        let _ = server.join();

        assert!(saw_started, "goal.started must be published");
        let ev = terminal.expect("a terminal event");
        assert_eq!(
            ev.kind, "goal.completed",
            "web goal must complete, not fail: {ev:?}"
        );
        assert_eq!(ev.payload["strategy"], "heuristic");
        let result = ev.payload["result"].as_object().expect("result object");
        assert_eq!(
            result["kind"], "text",
            "structured results, not an error: {result:?}"
        );
        assert_eq!(result["tool"], "web", "served by the web-search tool");
        let text = result["text"].as_str().unwrap_or_default();
        let output: serde_json::Value = serde_json::from_str(text).expect("result text is JSON");
        let results = output["results"].as_array().expect("results array");
        assert_eq!(results.len(), 2, "both mock hits parsed: {output:?}");
        assert_eq!(output["query"], "latest Rust async docs");
        // The mock server saw the real derived query (not the whole sentence)
        // among the requests it served.
        let reqs = requests.lock().unwrap().clone();
        assert!(
            reqs.iter()
                .any(|r| r.contains("latest%20Rust%20async%20docs")),
            "query was sent: {reqs:?}"
        );
    }

    /// Regression: a capability with NO registered executor must fail the goal
    /// explicitly (`no tool for capability: …`) instead of silently falling
    /// back to the shell. SearchMemory is the planner's safe default and the
    /// runner advertises it in the world state but registers no tool for it —
    /// so an unclassified goal must surface that honesty, never a bogus shell
    /// attempt.
    #[tokio::test]
    async fn unregistered_capability_fails_explicitly_not_shell_fallback() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "explain the provider routing architecture" }),
        ))
        .await
        .expect("publish");

        let mut saw_started = false;
        let mut terminal = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.started" => saw_started = true,
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev);
                    break;
                }
                _ => {}
            }
        }

        assert!(saw_started, "goal.started must be published");
        let ev = terminal.expect("a terminal event");
        let result = ev.payload["result"].as_object().expect("result object");
        assert_eq!(
            result["kind"], "error",
            "an unregistered capability fails explicitly: {result:?}"
        );
        let text = result["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("no tool for capability"),
            "the failure names the missing tool: {text}"
        );
        assert_ne!(
            result["tool"], "terminal",
            "must never silently fall back to ExecuteCommand: {result:?}"
        );
    }

    /// The completed payload contract: served route decision surfaces as
    /// provider/model with strategy=llm; a None served decision reports the
    /// heuristic strategy and no provider. The frontend renders these fields
    /// as the task-card detail.
    #[test]
    fn completed_payload_carries_channels_and_served_route() {
        use friday_goal_execution::{EvidenceActionRecord, EvidenceTrail, ExecutionState};

        let mut report = ExecutiveReport::default();
        report.plan_executed = true;
        report.evidence_trail = EvidenceTrail {
            goal: "Check the git status of the workspace repository".into(),
            records: vec![EvidenceActionRecord {
                step: 0,
                instruction: "gather evidence: git (git.status)".into(),
                capability: "git.status".into(),
                channel: "git".into(),
                execution_state: ExecutionState::Verified,
                verified: true,
                tool_success: true,
                duration_ms: 4,
            }],
        };

        // The fabric served a model → provider/model + strategy=llm.
        let served = (
            "openrouter".to_string(),
            "openrouter/nvidia/nemotron:free".to_string(),
        );
        let p = completed_payload("g", "id-1", &report, Some(&served));
        assert_eq!(p["strategy"], "llm");
        assert_eq!(p["provider"], "openrouter");
        assert_eq!(p["model"], "openrouter/nvidia/nemotron:free");
        assert_eq!(p["evidence_verified"], 1);
        let channels = p["channels"].as_array().expect("channels");
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0]["channel"], "git");
        assert_eq!(channels[0]["verified"], json!(true));
        // No executive result → the payload still carries a well-formed,
        // explicit empty result — never an invented answer.
        assert_eq!(p["result"]["kind"], "empty");
        assert!(p["result"]["text"].as_str().unwrap_or_default().is_empty());

        // The model errored / no provider → heuristic strategy, no provider key.
        let p = completed_payload("g", "id-2", &report, None);
        assert_eq!(p["strategy"], "heuristic");
        assert!(p.get("provider").is_none());
        assert!(p.get("model").is_none());
        // The channels are still the evidence-gated plan's channels.
        assert_eq!(p["channels"][0]["channel"], "git");
    }

    /// The completed payload contract carries the goal's canonical result
    /// verbatim: the kind, the renderable text (the actual execution output),
    /// and the producing tool — the exact JSON the frontend `parseResult`
    /// reads across the WebSocket boundary.
    #[test]
    fn completed_payload_carries_the_goal_result() {
        use friday_goal_execution::{GoalResult, ToolResult};

        let mut report = ExecutiveReport::default();
        report.plan_executed = true;
        report.result = Some(
            GoalResult::from_tool_result(&ToolResult::success(
                "Here are the files in the project:\n- Cargo.toml\n- frontend/\n- services/",
            ))
            .with_tool("filesystem"),
        );

        let p = completed_payload("List the files in this project", "id-3", &report, None);
        let result = p["result"].as_object().expect("result object");
        assert_eq!(result["kind"], "text");
        assert!(result["text"]
            .as_str()
            .unwrap_or_default()
            .contains("Here are the files in the project:"));
        assert_eq!(result["tool"], "filesystem");
        assert_eq!(result["success"], true);
        // The existing metadata stays intact alongside the result.
        assert!(p["phases"].is_number());
        assert!(p["evidence_verified"].is_number());
        assert!(p["channels"].is_array());
    }

    /// A goal whose execution produces a real result reaches `goal.completed`
    /// with that result attached: "list the files" runs the real filesystem
    /// adapter and the listing survives to the completed payload, with the
    /// phase/evidence metadata intact. This is the Rust → WS boundary contract
    /// the portal's `parseResult` decodes.
    #[tokio::test]
    async fn textual_result_reaches_goal_completed() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "List the files in this project" }),
        ))
        .await
        .expect("publish");

        let mut completed: Option<serde_json::Value> = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.completed" {
                completed = Some(ev.payload);
                break;
            }
        }

        let payload = completed.expect("goal.completed must be published");
        let result = payload["result"].as_object().expect("result object");
        assert_eq!(
            result["kind"], "text",
            "the listing is a text result: {result:?}"
        );
        let text = result["text"].as_str().expect("result text");
        assert!(
            !text.is_empty(),
            "the listing text must not be empty: {result:?}"
        );
        assert!(
            text.contains("entries") || text.contains("Cargo.toml"),
            "the result is the real directory listing: {text}"
        );
        // Metadata is preserved alongside the result — the card still shows
        // phases/evidence, just no longer instead of the answer.
        assert!(
            payload["phases"].as_u64().unwrap_or(0) >= 10,
            "the executive ran its full cycle: {payload:?}"
        );
        assert!(payload.get("evidence_verified").is_some());
        assert!(payload["channels"].is_array());
    }

    #[test]
    fn filename_token_extracts_the_named_file() {
        assert_eq!(
            filename_token("Read the contents of planning.rs"),
            Some("planning.rs".to_string())
        );
        assert_eq!(
            filename_token("show config.toml"),
            Some("config.toml".to_string())
        );
        // No dotted filename → none.
        assert_eq!(filename_token("list the files in this project"), None);
        assert_eq!(filename_token("run git status"), None);
        // Paths stay whole so they can join the working dir.
        assert_eq!(
            filename_token("read src/main.rs"),
            Some("src/main.rs".to_string())
        );
    }

    #[test]
    fn web_query_strips_routing_verbs_keeps_the_search_text() {
        assert_eq!(
            web_query_from_instruction("Search the web for the latest Rust async docs"),
            "latest Rust async docs"
        );
        assert_eq!(
            web_query_from_instruction("search for tokio docs"),
            "tokio docs"
        );
        assert_eq!(
            web_query_from_instruction("Find the rust book"),
            "rust book"
        );
        // No routing verb → pass through untouched.
        assert_eq!(web_query_from_instruction("rust async"), "rust async");
    }

    #[test]
    fn command_from_instruction_strips_the_leading_verb() {
        assert_eq!(command_from_instruction("Run git status"), "git status");
        assert_eq!(command_from_instruction("run cargo test"), "cargo test");
        assert_eq!(
            command_from_instruction("Execute the build script"),
            "the build script"
        );
        // No leading verb → pass through untouched (a non-command goal should
        // never reach this tool anyway).
        assert_eq!(command_from_instruction("git status"), "git status");
    }

    #[test]
    fn search_params_derives_query_and_extension() {
        let p = |ins: &str| search_params(ins).unwrap_or_default();

        // "Find all Rust files" → the language word survives stopword removal
        // and maps to the `.rs` extension so the search finds real files.
        let rust = p("Find all Rust files");
        assert_eq!(rust["query"], "rust");
        assert_eq!(rust["ext"], "rs");

        // Generic phrasing keeps only the substantive noun.
        let config = p("find config files");
        assert_eq!(config["query"], "config");
        assert!(config.get("ext").is_none());

        // JSON request keeps the bare format hint.
        let json_q = p("search for json files");
        assert_eq!(json_q["query"], "json");
        assert_eq!(json_q["ext"], "json");

        // "containing X" is a CONTENT search: the substantive noun becomes
        // `content` (matched against file contents), the language word an ext
        // filter — the adapter can then find the symbol ExpertRouter in real
        // `.rs` files instead of hunting for it in filenames.
        let containing = p("Find all Rust files containing ExpertRouter");
        assert_eq!(containing["content"], "expertrouter");
        assert_eq!(containing["ext"], "rs");
        assert!(containing.get("query").is_none());

        // Content intent whose only term is a language word: the word is the
        // needle, no extension filter.
        let rust_only = p("files containing rust");
        assert_eq!(rust_only["content"], "rust");
        assert!(rust_only.get("ext").is_none());

        // A pure-stopword instruction carries no query — the search then lists
        // everything (the adapter's default), which is the sane fallback.
        assert!(search_params("find all the files").is_none());
    }

    #[tokio::test]
    async fn world_advertises_filesystem_tree_search_and_memory() {
        let bus = EventBus::new();
        let runner = GoalRunner::new(bus.clone());
        let world = runner.workspace.get_world_state().expect("world state set");
        for cap in [
            Capability::Custom("filesystem.tree".into()),
            Capability::Custom("filesystem.search".into()),
            Capability::SearchMemory,
        ] {
            assert!(
                world.has_tool(&cap),
                "the world must advertise {cap:?} so tree/search/memory plans are feasible"
            );
        }
        // The tool registry actually serves the custom ids it advertises.
        let tool = FilesystemCapabilityTool {
            adapter: FilesystemAdapter::new(),
        };
        let caps = tool.capabilities();
        for id in ["filesystem.tree", "filesystem.search"] {
            assert!(
                caps.contains(&Capability::Custom(id.into())),
                "the filesystem tool must serve {id}"
            );
        }
    }

    /// A "show the whole project structure" goal routes to the filesystem.tree
    /// custom id and completes with a real tree — the wiring from direct goal
    /// → capability → adapter verb is intact end to end.
    #[tokio::test]
    async fn tree_goal_reaches_goal_completed_with_a_tree() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Show the whole project structure" }),
        ))
        .await
        .expect("publish");

        let mut completed: Option<serde_json::Value> = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.completed" {
                completed = Some(ev.payload);
                break;
            }
        }

        let payload = completed.expect("goal.completed must be published");
        let result = payload["result"].as_object().expect("result object");
        assert_eq!(result["kind"], "text");
        let text = result["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("tree") && text.contains("Cargo.toml"),
            "the result is a real directory tree: {text}"
        );
    }

    /// Drive one goal to its terminal event and return the started + terminal
    /// payloads (panics on timeout), mirroring the existing test pattern.
    async fn drive_goal(
        bus: &EventBus,
        rx: &mut tokio::sync::broadcast::Receiver<Event>,
        payload: serde_json::Value,
    ) -> (serde_json::Value, serde_json::Value) {
        bus.publish(Event::new("plan.goal", "mobile", payload))
            .await
            .expect("publish");
        let mut started = None;
        let mut terminal = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.started" => started = Some(ev.payload),
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev.payload);
                    break;
                }
                _ => {}
            }
        }
        (
            started.expect("goal.started must be published"),
            terminal.expect("a terminal event must be published"),
        )
    }

    /// S1 acceptance: the session id survives ingress end-to-end (runner sees
    /// the same id it published for), the same session stays continuous (the
    /// runner keys context by it — the turn increments per session), and
    /// different sessions are isolated (each starts fresh at turn 1).
    #[tokio::test]
    async fn sessions_are_continuous_and_isolated() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        let (s1_started, s1_done) = drive_goal(
            &bus,
            &mut rx,
            json!({
                "goal": "Check the git status of the workspace repository",
                "session_id": "sess-a",
            }),
        )
        .await;
        assert_eq!(s1_started["session_id"], "sess-a");
        assert_eq!(s1_started["session_turn"], 1);
        assert_eq!(s1_done["session_id"], "sess-a");
        assert_eq!(s1_done["session_turn"], 1);
        assert_eq!(s1_done["result"]["kind"], "text", "turn 1 completes");

        // Same session, second goal: continuous — turn 2 under the same id.
        let (s2_started, s2_done) = drive_goal(
            &bus,
            &mut rx,
            json!({
                "goal": "Check the git status of the workspace repository",
                "session_id": "sess-a",
            }),
        )
        .await;
        assert_eq!(s2_started["session_id"], "sess-a");
        assert_eq!(s2_started["session_turn"], 2);
        assert_eq!(s2_done["session_id"], "sess-a");
        assert_eq!(s2_done["session_turn"], 2);
        assert_eq!(s2_done["result"]["kind"], "text", "turn 2 completes");

        // Different session: isolated — starts at turn 1 under its own id, and
        // the runner holds both sessions' contexts independently.
        let (s3_started, s3_done) = drive_goal(
            &bus,
            &mut rx,
            json!({
                "goal": "Check the git status of the workspace repository",
                "session_id": "sess-b",
            }),
        )
        .await;
        assert_eq!(s3_started["session_id"], "sess-b");
        assert_eq!(s3_started["session_turn"], 1, "sess-b starts fresh");
        assert_eq!(s3_done["session_id"], "sess-b");
        assert_eq!(s3_done["session_turn"], 1);
    }

    /// S1 acceptance: backward compatibility — a `plan.goal` without a
    /// `session_id` (older portal builds) still runs, under the fixed default
    /// session identity, and stays continuous across that session's goals.
    #[tokio::test]
    async fn goal_without_session_id_runs_under_default_session() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        let (started, done) = drive_goal(
            &bus,
            &mut rx,
            json!({ "goal": "Check the git status of the workspace repository" }),
        )
        .await;
        assert_eq!(started["session_id"], DEFAULT_SESSION_ID);
        assert_eq!(started["session_turn"], 1);
        assert_eq!(done["session_id"], DEFAULT_SESSION_ID);
        assert_eq!(done["result"]["kind"], "text");

        let (started2, _) = drive_goal(
            &bus,
            &mut rx,
            json!({ "goal": "Check the git status of the workspace repository" }),
        )
        .await;
        assert_eq!(started2["session_id"], DEFAULT_SESSION_ID);
        assert_eq!(
            started2["session_turn"], 2,
            "default session stays continuous"
        );
    }

    /// S2 acceptance: persistent memory write + recall. Telling FRIDAY a fact,
    /// "closing" it (a brand-new runner, fresh Executive, on the same journal),
    /// and asking about it later recalls the fact from durable memory — the
    /// "use it today, close it, reopen it tomorrow, it still knows" criterion.
    #[tokio::test]
    async fn memory_recall_surfaces_prior_fact_across_restart() {
        let path =
            std::env::temp_dir().join(format!("friday-mobile-memory-{}", uuid::Uuid::new_v4()));
        let journal = MemoryJournal::open(&path).expect("open journal");

        // Turn 1, "today": the fact is admitted and journaled.
        let bus1 = EventBus::new();
        let mut rx1 = bus1.subscribe();
        GoalRunner::with_memory(bus1.clone(), journal.clone()).start();
        let (_, done1) = drive_goal(
            &bus1,
            &mut rx1,
            json!({ "goal": "My main project is FRIDAY" }),
        )
        .await;
        assert_eq!(done1["result"]["kind"], "text");
        assert!(
            !journal.is_empty(),
            "the fact was written into durable memory (journal len {})",
            journal.len()
        );

        // "Tomorrow": a fresh runner (new Executive, new session state) on the
        // same journal must still know what mattered.
        let bus2 = EventBus::new();
        let mut rx2 = bus2.subscribe();
        GoalRunner::with_memory(bus2.clone(), journal.clone()).start();
        let (_, done2) = drive_goal(
            &bus2,
            &mut rx2,
            json!({ "goal": "What was that project I told you about?" }),
        )
        .await;
        let text = done2["result"]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("recalled") && text.contains("FRIDAY"),
            "the restarted runner recalls the fact from memory: {text}"
        );

        let _ = std::fs::remove_dir_all(&path);
    }

    /// S2 acceptance: an explicit "remember X" writes the fact to memory, and a
    /// later question about it in the same session recalls it.
    #[tokio::test]
    async fn remember_goal_writes_fact_and_later_goal_recalls_it() {
        let path =
            std::env::temp_dir().join(format!("friday-mobile-remember-{}", uuid::Uuid::new_v4()));
        let journal = MemoryJournal::open(&path).expect("open journal");
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        GoalRunner::with_memory(bus.clone(), journal.clone()).start();

        let (_, done1) = drive_goal(
            &bus,
            &mut rx,
            json!({ "goal": "Remember that my main project is FRIDAY" }),
        )
        .await;
        let text1 = done1["result"]["text"].as_str().unwrap_or_default();
        assert!(
            text1.contains("remembered"),
            "the remember goal acknowledges the write: {text1}"
        );
        assert!(
            !journal.is_empty(),
            "the remembered fact was written to durable memory (journal len {})",
            journal.len()
        );

        let (_, done2) = drive_goal(
            &bus,
            &mut rx,
            json!({ "goal": "What was that project I told you about?" }),
        )
        .await;
        let text2 = done2["result"]["text"].as_str().unwrap_or_default();
        assert!(
            text2.contains("recalled") && text2.contains("FRIDAY"),
            "the written fact is recalled: {text2}"
        );

        let _ = std::fs::remove_dir_all(&path);
    }

    /// S1 continuity seam: the goal the Executive plans on is composed from the
    /// session's prior turn — but only when a prior turn exists, so the first
    /// goal of a session is never polluted with context. S2: the `[memory]`
    /// block joins it when recall returned entries.
    #[test]
    fn compose_goal_with_context_embeds_prior_turn_only_when_present() {
        let fresh = SessionContext::new();
        assert_eq!(
            compose_goal_with_context("Read the file foo.txt", &fresh, &[]),
            "Read the file foo.txt"
        );

        let mut prior = SessionContext::new();
        prior.turn = 1;
        prior.last_goal = "List the files in this project".into();
        prior.last_result = "src/main.rs, src/lib.rs".into();
        let composed = compose_goal_with_context("Show me the second file", &prior, &[]);
        assert!(composed.starts_with("Show me the second file"));
        assert!(composed.contains("[session context]"));
        assert!(composed.contains("previous goal: List the files in this project"));
        assert!(composed.contains("previous result: src/main.rs, src/lib.rs"));

        // The memory block appears even on a session's first turn — memory
        // spans sessions, so there is nothing to anchor to but the recall.
        let recalled = vec!["goal: My main project is FRIDAY\nresult: acknowledged".to_string()];
        let composed = compose_goal_with_context("What was that project?", &fresh, &recalled);
        assert!(composed.contains("[memory]"));
        assert!(composed.contains("goal: My main project is FRIDAY"));

        let mut big = SessionContext::new();
        big.turn = 1;
        big.last_goal = "g".repeat(MAX_CONTEXT_CHARS * 2);
        big.last_result = "r".repeat(MAX_CONTEXT_CHARS * 2);
        let composed = compose_goal_with_context("next", &big, &[]);
        assert!(
            composed.len() < MAX_CONTEXT_CHARS * 3,
            "composed context stays bounded: {}",
            composed.len()
        );
    }

    /// The per-session map stays bounded: LRU eviction drops the oldest
    /// sessions first.
    #[test]
    fn session_map_is_bounded_by_lru_eviction() {
        let sessions: Mutex<HashMap<String, SessionContext>> = Mutex::new(HashMap::new());
        for i in 0..(MAX_SESSIONS + 8) {
            let mut ctx = SessionContext::new();
            ctx.last_seen_ms = i as u64;
            sessions.lock().unwrap().insert(format!("sess-{i}"), ctx);
        }
        evict_lru(&sessions, MAX_SESSIONS);
        let map = sessions.lock().unwrap();
        assert_eq!(map.len(), MAX_SESSIONS);
        assert!(map.get("sess-0").is_none(), "oldest evicted");
        assert!(map.get("sess-7").is_none(), "oldest evicted");
        assert!(
            map.get(&format!("sess-{}", MAX_SESSIONS + 7)).is_some(),
            "newest survive"
        );
    }
}
