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
    ActionAdapter, ActionRequest, BrowserAdapter, CodeIntelAdapter, DocsAdapter, FilesystemAdapter,
    GitAdapter, TerminalAdapter, WebAdapter, WebAgentAdapter,
};
use friday_core::event::{Event, EventBus};
use friday_goal_execution::{
    Action, AdapterTool, AdmissionVerdict, Capability, CognitiveBlackboard, CognitiveEpisode,
    ExecutionContext, Executive, ExecutiveReport, GoalResult, GoalResultKind, MemoryJournal,
    Observer, ResolvingWebTool, Tool, ToolResult, WebFetchProvider, WebSearchProvider, WorldState,
    JINA_READER_PREFIX,
};
use friday_intelligence::InferenceFabric;
use friday_investigation::{InvestigationEngine, LlmStrategyClient};
#[cfg(any(windows, target_os = "linux", test))]
use friday_sensors::SensorLoop;
#[cfg(windows)]
use friday_sensors::{screen::WindowsScreenSensor, windows_focus::WindowsFocusSensor};
use friday_windows::WindowsAdapter;
use serde_json::json;

use crate::voice::VoiceConfig;
use crate::web_search::{append_sources, ChainedWebSearcher, SearchResponse, WebSearcher};

/// Shared confirmation gate for gated side-effecting tools.
#[derive(Debug, Default)]
pub(crate) struct ConfirmationGate {
    pub(crate) confirm: Option<String>,
    pub(crate) pending: Option<crate::shell_state::NeedConfirmation>,
}

impl ConfirmationGate {
    pub(crate) fn check_or_record(
        &mut self,
        tool: &str,
        exact_action: &str,
    ) -> Result<(), ToolResult> {
        if let Some(c) = &self.confirm {
            if c.trim() == exact_action.trim() {
                return Ok(());
            }
        }
        if self.pending.is_none() {
            self.pending = Some(crate::shell_state::NeedConfirmation {
                tool: tool.to_string(),
                action: exact_action.to_string(),
            });
        }
        Err(ToolResult::error(format!(
            "Confirm to run: {}",
            exact_action
        )))
    }
}

/// Real terminal tool for the built-in `ExecuteCommand` capability (the
/// planner's `infer_capability` classifies goals like "run cargo test" as
/// execute-command; the evidence seam only advertises `terminal.run/exec`
/// custom ids, so this tool bridges the built-in capability to the real
/// terminal adapter).
struct TerminalCommandTool {
    adapter: TerminalAdapter,
    gate: Arc<Mutex<ConfirmationGate>>,
}

/// Real filesystem tool for the built-in `ListDirectory` / `ReadFile` /
/// `WriteFile` capabilities (the planner's `infer_capability` classifies goals
/// like "list the files in this project" as list-directory; the evidence seam
/// only advertises `filesystem.list_directory` custom ids, so this tool bridges
/// the built-in capabilities to the real filesystem adapter).
struct FilesystemCapabilityTool {
    adapter: FilesystemAdapter,
    gate: Arc<Mutex<ConfirmationGate>>,
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
        let exact = command_from_instruction(&action.instruction);
        if let Err(err) = self
            .gate
            .lock()
            .unwrap()
            .check_or_record("terminal", &exact)
        {
            return err;
        }
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "terminal",
                "run",
                json!({ "command": exact }),
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
            } else if verb == "write_file" {
                let p = filename_token(&action.instruction).unwrap_or_else(|| cwd.clone());
                params["path"] = json!(p);
            } else {
                params["path"] = json!(cwd);
            }
        }
        if verb == "write_file" {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let exact = format!("write {}", path);
            if let Err(err) = self
                .gate
                .lock()
                .unwrap()
                .check_or_record("filesystem.write", &exact)
            {
                return err;
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
        vec![
            Capability::SearchWeb,
            // Learned workflow policies reference the channel id they were
            // learned from (evidence actions carry `web.search`); a goal-level
            // `web.search` action must execute a real search here, never be
            // refused by the `evidence:web` wrapper (the evidence dispatch
            // still routes `gather evidence:` actions to `evidence:web`).
            Capability::Custom("web.search".into()),
        ]
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

/// Real local-docs search tool for the `docs.search` skill id. Learned
/// workflow policies reference the channel ids they were learned from, so a
/// `web.search → docs.search` policy must execute a real docs search as a goal
/// action — the `evidence:docs` wrapper would refuse the non-evidence
/// instruction. Registered ahead of `evidence:docs`; evidence docs.search
/// actions still resolve to `evidence:docs` via the evidence dispatch.
struct DocsSearchCapabilityTool {
    adapter: DocsAdapter,
}

#[async_trait]
impl Tool for DocsSearchCapabilityTool {
    fn name(&self) -> &str {
        "docs"
    }

    fn description(&self) -> &str {
        "Searches the local documentation and returns matching files (docs.search capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom("docs.search".into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "docs",
                "search",
                json!({ "query": action.instruction.trim() }),
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

/// Real desktop host tool for the `windows.open` capability (S4). The planner
/// classifies "open notepad" / "launch chrome" as `windows.open`; this tool
/// bridges the capability to the WindowsAdapter, which forwards the intent to
/// the C# host over the named pipe (Windows) or to Hyprland (Linux). When the
/// host is not running the transport returns HostUnavailable — an honest
/// failure, never a fake launch.
struct WindowsOpenTool {
    adapter: WindowsAdapter,
    gate: Arc<Mutex<ConfirmationGate>>,
}

#[async_trait]
impl Tool for WindowsOpenTool {
    fn name(&self) -> &str {
        "windows"
    }

    fn description(&self) -> &str {
        "Opens desktop applications on this machine (windows.open capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom("windows.open".into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let exact = app_from_instruction(&action.instruction);
        if let Err(err) = self
            .gate
            .lock()
            .unwrap()
            .check_or_record("windows.open", &exact)
        {
            return err;
        }
        let res = self
            .adapter
            .execute(ActionRequest::new(
                "windows",
                "open",
                json!({ "path": exact }),
            ))
            .await;
        ToolResult {
            success: res.success,
            output: serde_json::to_string_pretty(&res.output).unwrap_or_default(),
            error: res.error.as_deref().map(|e| open_failure(&exact, e)),
            duration_ms: res
                .metadata
                .get("latency_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            metadata: json!(res.metadata),
        }
    }
}

/// Real browser tool for the `web.open` capability (S4). The planner classifies
/// "open example.com" as `web.open`; this tool bridges the capability to the
/// BrowserAdapter (CDP-backed Chrome). Only the safe `web.open` verb is
/// advertised — the adapter's high-risk `web.evaluate` stays unadvertised
/// (denied by default).
struct BrowserCapabilityTool {
    adapter: BrowserAdapter,
}

#[async_trait]
impl Tool for BrowserCapabilityTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Opens URLs in a browser (web.open capability)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom("web.open".into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        let Some(url) = url_from_instruction(&action.instruction) else {
            return ToolResult::error(
                "no URL found in the goal — say which address to open (e.g. \"open example.com\")",
            );
        };
        let res = self
            .adapter
            .execute(ActionRequest::new("web", "open", json!({ "url": url })))
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
/// Whether the Observe phase may read the focused window's pixels (to notice
/// that its content changed). Off unless the person opts in with
/// `FRIDAY_OBSERVE_PIXELS=1`: screen capture needs consent, and the window's
/// title and geometry are observed either way.
#[cfg(any(windows, target_os = "linux"))]
fn observe_pixels() -> bool {
    std::env::var("FRIDAY_OBSERVE_PIXELS").is_ok_and(|v| v == "1")
}

fn command_from_instruction(instruction: &str) -> String {
    // Only the user's own words: selected text and context blocks never run.
    let trimmed = friday_goal_execution::planning::goal_segment(instruction);
    for prefix in ["run ", "Run ", "RUN ", "execute ", "Execute ", "EXECUTE "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// The desktop application a `windows.open` goal names, resolved to an
/// executable the C# host's CreateProcess can launch (via PATH or App Paths).
/// Known aliases map to their exe name; bare or unknown names get `.exe`
/// appended. "Open the calculator app" → `calc.exe`; "launch chrome" →
/// `chrome.exe`; "open notepad" → `notepad.exe`.
fn app_from_instruction(instruction: &str) -> String {
    // The goal's own line: a session-context block may follow it.
    let trimmed = instruction.lines().next().unwrap_or("").trim();
    let polite = trimmed
        .strip_prefix("please ")
        .or_else(|| trimmed.strip_prefix("Please "))
        .unwrap_or(trimmed);
    let rest = ["open ", "Open ", "launch ", "Launch "]
        .iter()
        .find_map(|p| polite.strip_prefix(p))
        .unwrap_or(polite);
    let mut name = rest.trim().to_lowercase();
    // "open brave on this device", "open notepad for me please"
    for tail in [
        " please",
        " for me",
        " on this device",
        " on this computer",
        " on this pc",
        " on this laptop",
        " on this machine",
        " on my computer",
        " on my pc",
    ] {
        if let Some(stripped) = name.strip_suffix(tail) {
            name = stripped.trim().to_string();
        }
    }
    for prefix in ["the ", "a ", "an ", "my "] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped.to_string();
            break;
        }
    }
    for suffix in [" app", " application", "."] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_string();
            break;
        }
    }
    let known = match name.as_str() {
        "notepad" | "text editor" => Some("notepad.exe"),
        "chrome" | "google chrome" | "browser" | "web browser" => Some("chrome.exe"),
        "edge" | "microsoft edge" => Some("msedge.exe"),
        "explorer" | "file explorer" => Some("explorer.exe"),
        "cmd" | "command prompt" => Some("cmd.exe"),
        "powershell" => Some("powershell.exe"),
        "terminal" | "windows terminal" => Some("wt.exe"),
        "calculator" | "calc" => Some("calc.exe"),
        "vscode" | "visual studio code" | "vs code" => Some("code.exe"),
        "word" | "microsoft word" => Some("winword.exe"),
        "excel" | "microsoft excel" => Some("excel.exe"),
        _ => None,
    };
    if let Some(exe) = known {
        return exe.to_string();
    }
    // "brave browser" is the program "brave"; the host looks the name up in
    // App Paths and the Start menu, so a longer name ("microsoft teams") is
    // passed whole instead of cut to its first word.
    if let Some(stripped) = name.strip_suffix(" browser") {
        name = stripped.trim().to_string();
    }
    if name.is_empty() {
        trimmed.to_string()
    } else if name.ends_with(".exe") || name.contains(' ') {
        name
    } else {
        format!("{name}.exe")
    }
}

/// What to tell the person when the host could not open `app`.
fn open_failure(app: &str, error: &str) -> String {
    match error {
        "AppNotFound" => format!(
            "I couldn't find an app called \"{}\" on this PC. Try its name as it appears in the Start menu.",
            app.trim_end_matches(".exe")
        ),
        "HostUnavailable" => {
            "FRIDAY's Windows helper isn't running (restart FRIDAY from its tray icon)".to_string()
        }
        other => other.strip_prefix("Unknown:").unwrap_or(other).to_string(),
    }
}

/// The URL a `web.open` goal names: a scheme, a `www.` host, or a bare domain
/// with a known suffix (→ https://). Anything else returns `None` so the tool
/// can fail honestly instead of guessing.
fn url_from_instruction(instruction: &str) -> Option<String> {
    let tlds = [
        "com", "org", "net", "io", "dev", "co", "ai", "me", "app", "to", "gov", "edu", "xyz",
    ];
    for tok in instruction.split_whitespace() {
        let t = tok.trim_matches(|c: char| c.is_ascii_punctuation());
        if t.contains("://") {
            return Some(t.to_string());
        }
        if t.starts_with("www.") {
            return Some(format!("https://{t}"));
        }
        if let Some((_host, tld)) = t.rsplit_once('.') {
            if tlds.contains(&tld) {
                return Some(format!("https://{t}"));
            }
        }
    }
    None
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

/// A word of the instruction that is a file in `cwd` without an extension, like
/// `Dockerfile` in "read the Dockerfile".
fn bare_file_token(instruction: &str, cwd: &str) -> Option<String> {
    instruction
        .split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| c.is_ascii_punctuation()))
        .find(|t| !t.is_empty() && std::path::Path::new(cwd).join(t).is_file())
        .map(String::from)
}

/// Resolve the path a read goal names: the file as given relative to the
/// working directory, else the first filesystem.search hit by name, else an
/// honest error instead of reading a directory.
async fn read_path_from_instruction(
    adapter: &FilesystemAdapter,
    instruction: &str,
    cwd: &str,
) -> Result<String, String> {
    let Some(name) = filename_token(instruction).or_else(|| bare_file_token(instruction, cwd))
    else {
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
/// `goal.started` / `goal.completed` / `goal.failed` / `goal.cancelled` /
/// `goal.paused` events back to the bus so the portal chat shows real FRIDAY
/// activity instead of a silent ack.
///
/// **Human control (S3):** a dedicated control subscriber (spawned by
/// [`Self::start`]) consumes `task.cancel` / `task.pause` / `task.approve`
/// *while* a goal cycle is running. Because the goal loop holds the executive
/// lock across the whole cycle, control is cooperative: the control loop flips
/// shared [`AtomicBool`] flags, and the Executive's cycle checks them at
/// phase/action boundaries, stops, and reports — so the runner publishes the
/// real state transitions `goal.cancelled` / `goal.paused`, never a silent
/// ack. `task.approve` approves a held episode (admission `Pending`) into
/// memory and publishes `goal.approved`.
pub struct GoalRunner {
    bus: EventBus,
    executive: Arc<tokio::sync::Mutex<Executive>>,
    workspace: Arc<CognitiveBlackboard>,
    /// The fabric backing the LLM investigation. Retained (one Arc clone) so
    /// the runner can read the routing decision after each cycle and surface
    /// on `goal.completed` which provider/model actually answered. `None` for
    /// the heuristic-only runner (`Self::new`).
    fabric: Option<Arc<InferenceFabric>>,
    /// The model that writes the reply once a goal has run (the answer step,
    /// see [`compose_answer`]): the fabric when there is one, so a question
    /// gets an answer and not just a tool's raw output. `None` (heuristic
    /// runners) keeps the tool output as the result.
    answerer: Option<Arc<dyn LlmStrategyClient>>,
    /// Per-session conversational context (`session_id` → context), bounded by
    /// [`MAX_SESSIONS`] with LRU eviction. Locked only in short critical
    /// sections — never held across an await.
    sessions: Mutex<HashMap<String, SessionContext>>,
    /// The durable memory journal this runner writes through (shared with the
    /// Executive's `EpisodeStore` and the recall tool — one store, one
    /// journal). `None` = memory stays in-memory only (heuristic test runners
    /// and any caller that does not opt into persistence).
    journal: Option<Arc<MemoryJournal>>,
    /// The cancellation flag the Executive's cycle checks (S3). The control
    /// subscriber flips it on `task.cancel`; a fresh goal clears it.
    abort: Arc<std::sync::atomic::AtomicBool>,
    /// The pause flag the Executive's cycle checks (S3). Flipped by the
    /// control subscriber on `task.pause`; cleared by a fresh goal.
    pause: Arc<std::sync::atomic::AtomicBool>,
    /// The goal currently being run (`goal.started.id`), for the control
    /// subscriber to match `task.cancel`/`task.pause` against. `None` when no
    /// goal is in flight.
    in_flight: Arc<Mutex<Option<InFlightGoal>>>,
    /// Shared confirmation gate for gated tools (terminal, windows.open, filesystem.write).
    confirmation_gate: Arc<Mutex<ConfirmationGate>>,
    /// Pluggable web search provider for goals requesting web search.
    web_searcher: Arc<dyn WebSearcher>,
    /// Private mode (FRIDAY_PRIVATE): nothing leaves the machine, so a web
    /// search is never made.
    private_mode: bool,
}

/// The in-flight goal the control subscriber matches `task.cancel` /
/// `task.pause` against (the id flows in `goal.started`). Only the id is
/// needed — it is unique per run, so a stale control command can never
/// match a newer goal.
#[derive(Debug)]
struct InFlightGoal {
    id: String,
}

/// A handle to inspect the status of the [`GoalRunner`].
#[derive(Clone, Debug)]
pub struct GoalRunnerHandle {
    in_flight: Arc<Mutex<Option<InFlightGoal>>>,
}

impl GoalRunnerHandle {
    /// Returns true if no goal is currently in flight.
    pub fn is_idle(&self) -> bool {
        match self.in_flight.lock() {
            Ok(g) => g.is_none(),
            Err(p) => p.into_inner().is_none(),
        }
    }
}

impl GoalRunner {
    /// The assistant loop with the deterministic heuristic investigation
    /// strategy — hermetic (no model calls), used by tests.
    pub fn new(bus: EventBus) -> Self {
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
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
            None,
            None,
            None,
        )
    }

    /// Use `client` for the answer step (tests script it; the daemon's runners
    /// already use their fabric).
    pub fn with_answerer(mut self, client: Arc<dyn LlmStrategyClient>) -> Self {
        self.answerer = Some(client);
        self
    }

    /// Override the web searcher (e.g. for hermetic testing).
    pub fn with_private_mode(mut self, enabled: bool) -> Self {
        self.private_mode = enabled;
        self
    }

    pub fn with_web_searcher(mut self, searcher: Arc<dyn WebSearcher>) -> Self {
        self.web_searcher = searcher;
        self
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
            None,
            None,
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
            None,
            None,
            None,
        )
    }

    /// The daemon's LLM-backed assistant loop with persistent memory: the
    /// fabric powers investigation and the shared `journal` makes memory survive
    /// restarts (close FRIDAY, reopen it tomorrow, it still knows). This is the
    /// live-host constructor: it also wires the S4 desktop adapters — the real
    /// Windows host (named pipe, `default` session matching the C# host's
    /// `FRIDAY_SESSION`), or on Linux the Hyprland host, and the browser
    /// (CDP-backed Chrome) — plus the S6 observation seam (the real screen +
    /// focus sensors).
    pub fn with_fabric_and_memory(
        bus: EventBus,
        fabric: Arc<InferenceFabric>,
        journal: Arc<MemoryJournal>,
        voice: VoiceConfig,
    ) -> Self {
        Self::with_investigation_engine(
            bus,
            InvestigationEngine::llm(Box::new(fabric.clone())),
            Some(fabric),
            None,
            Some(journal),
            #[cfg(windows)]
            Some(WindowsAdapter::for_session("default")),
            #[cfg(target_os = "linux")]
            Some(WindowsAdapter::for_hyprland()),
            #[cfg(not(any(windows, target_os = "linux")))]
            None,
            Some(BrowserAdapter::new()),
            Self::live_observer(&voice),
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
    /// `observer` attaches the S6 observation seam (the Executive polls it
    /// every Observe phase; `None` = no sensors, the exact pre-S6 behavior).
    #[allow(clippy::too_many_arguments)]
    fn with_investigation_engine(
        bus: EventBus,
        investigation: InvestigationEngine,
        fabric: Option<Arc<InferenceFabric>>,
        web_endpoint: Option<String>,
        journal: Option<Arc<MemoryJournal>>,
        windows: Option<WindowsAdapter>,
        browser: Option<BrowserAdapter>,
        observer: Option<Observer>,
    ) -> Self {
        let workspace = CognitiveBlackboard::new();
        let confirmation_gate = Arc::new(Mutex::new(ConfirmationGate::default()));
        let mut executive = Executive::new().with_investigation_engine(investigation);
        if let Some(observer) = observer {
            executive = executive.with_observer(observer);
        }
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
            gate: confirmation_gate.clone(),
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
        // The dedicated docs-search tool: the planner's direct goals carry the
        // `docs.search` custom id, and the registry resolves first-match.
        // Registered ahead of `evidence:docs` (which also advertises it) so a
        // goal-level docs search — direct or from a learned workflow policy —
        // is served by the dedicated tool, not refused by the evidence wrapper.
        executive.register_tool(Box::new(DocsSearchCapabilityTool {
            adapter: DocsAdapter::new(),
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
            gate: confirmation_gate.clone(),
        }));

        // S4 live-kernel adapters: the Windows host (named-pipe to the C# host)
        // and the browser (CDP-backed Chrome) are wired only when the daemon
        // passes them in — hermetic runners keep None so a test never talks to
        // a real pipe or launches Chrome.
        let has_windows = windows.is_some();
        let has_browser = browser.is_some();
        // Native control of windows, apps and media: FRIDAY OS (a Hyprland
        // session to drive) and Windows (through FRIDAY's host), with a model
        // to choose the steps. When it is there it also serves a plain "open
        // <app>" goal (the model finds the program), so the fixed
        // name-to-program tool is registered only without it.
        let has_desktop = fabric.is_some() && crate::desktop_tool::enabled();
        if let (Some(f), true) = (&fabric, has_desktop) {
            executive.register_tool(Box::new(
                crate::desktop_tool::DesktopControlTool::with_adapter(
                    crate::desktop_tool::default_adapter(),
                    f.clone() as Arc<dyn LlmStrategyClient>,
                    confirmation_gate.clone(),
                    executive.abort_token(),
                    crate::desktop_tool::audit_dir(),
                    crate::desktop_tool::default_ui(),
                ),
            ));
        }
        if let Some(adapter) = windows {
            if !has_desktop {
                executive.register_tool(Box::new(WindowsOpenTool {
                    adapter,
                    gate: confirmation_gate.clone(),
                }));
            }
        }
        if let Some(adapter) = browser {
            executive.register_tool(Box::new(BrowserCapabilityTool { adapter }));
        }

        // FRIDAY OS: system packages through the root helper. Registered
        // wherever the helper could be; every operation asks first, and with
        // the helper off the tool says how to turn it on.
        #[cfg(target_os = "linux")]
        let has_system = match &fabric {
            Some(f) if crate::system_tool::enabled() => {
                executive.register_tool(Box::new(crate::system_tool::SystemTool::new(
                    f.clone() as Arc<dyn LlmStrategyClient>,
                    Arc::new(crate::system_tool::DbusHelper),
                    confirmation_gate.clone(),
                    executive.abort_token(),
                    crate::desktop_tool::audit_dir(),
                )));
                true
            }
            _ => false,
        };
        #[cfg(not(target_os = "linux"))]
        let has_system = false;

        // Standing tasks ("every morning ..."): made from an ask, run by the
        // daemon's scheduler through `friday ask`.
        // Coding agents (opencode, agy) run on the person's request; on unless
        // they turned it off (`FRIDAY_AGENTS=0`).
        let has_agents = match &fabric {
            Some(f) if crate::agent_runs::enabled() => {
                executive.register_tool(Box::new(crate::agent_runs::AgentsTool::new(
                    f.clone() as Arc<dyn LlmStrategyClient>,
                    crate::agent_runs::runs_dir(),
                )));
                true
            }
            _ => false,
        };
        // Troubleshooting: read-only diagnostic commands, then an answer.
        let has_diagnose = match &fabric {
            Some(f) if crate::diagnose::enabled() => {
                executive.register_tool(Box::new(crate::diagnose::DiagnoseTool::new(
                    f.clone() as Arc<dyn LlmStrategyClient>
                )));
                true
            }
            _ => false,
        };
        let has_standing = match &fabric {
            Some(f) if crate::standing::enabled() => {
                executive.register_tool(Box::new(crate::standing::StandingTool::new(
                    f.clone() as Arc<dyn LlmStrategyClient>,
                    crate::standing::store_path(),
                )));
                true
            }
            _ => false,
        };

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
        if has_windows {
            world_state = world_state.with_tool(Capability::Custom("windows.open".into()));
        }
        if has_browser {
            world_state = world_state.with_tool(Capability::Custom("web.open".into()));
        }
        if has_desktop {
            world_state =
                world_state.with_tool(Capability::Custom(crate::desktop_tool::CAPABILITY.into()));
        }
        if has_system {
            world_state =
                world_state.with_tool(Capability::Custom(crate::system_tool::CAPABILITY.into()));
        }
        if has_agents {
            world_state =
                world_state.with_tool(Capability::Custom(crate::agent_runs::CAPABILITY.into()));
        }
        if has_diagnose {
            world_state =
                world_state.with_tool(Capability::Custom(crate::diagnose::CAPABILITY.into()));
        }
        if has_standing {
            world_state =
                world_state.with_tool(Capability::Custom(crate::standing::CAPABILITY.into()));
        }
        workspace.set_world_state(world_state);

        // The runner holds its own clones of the Executive's control flags so
        // the control subscriber can flip them while the cycle awaits inside
        // the exec lock (cooperative cancellation, S3).
        let abort = executive.abort_token();
        let pause = executive.pause_token();

        Self {
            bus,
            executive: Arc::new(tokio::sync::Mutex::new(executive)),
            workspace,
            answerer: fabric.clone().map(|f| f as Arc<dyn LlmStrategyClient>),
            fabric,
            sessions: Mutex::new(HashMap::new()),
            journal,
            abort,
            pause,
            in_flight: Arc::new(Mutex::new(None)),
            confirmation_gate,
            web_searcher: Arc::new(ChainedWebSearcher::default_chain()),
            private_mode: std::env::var("FRIDAY_PRIVATE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        }
    }

    /// The live-host observation seam (S6 + S8-C): the real Windows screen +
    /// focus sensors on one SensorLoop, plus — when voice is enabled — the
    /// real microphone behind Whisper, all folded into the blackboard
    /// WorldState on every Observe phase. Hermetic runners pass `None` so a
    /// test never touches a real window, screen, or mic. A missing
    /// microphone is honest silence: the sensor opens no capture device and
    /// the loop simply reports nothing.
    #[cfg(windows)]
    fn live_observer(voice: &VoiceConfig) -> Option<Observer> {
        let mut loop_ = SensorLoop::new();
        let screen = WindowsScreenSensor::new();
        loop_.add(Box::new(if observe_pixels() {
            screen
        } else {
            screen.without_pixels()
        }));
        loop_.add(Box::new(WindowsFocusSensor::new()));
        if let Some(mic) = voice.sensor() {
            loop_.add(Box::new(mic));
        }
        Some(Observer::new(loop_))
    }

    /// Linux: the same seam over Hyprland (screen + focus via `hyprctl`/`grim`),
    /// with commands run through the collector's runner so a daemon started
    /// before the compositor still finds the live session.
    #[cfg(target_os = "linux")]
    fn live_observer(voice: &VoiceConfig) -> Option<Observer> {
        use friday_context_collector::{CommandRunner, RealCommandRunner};
        use friday_sensors::hyprland::{CommandFn, HyprlandFocusSensor, HyprlandScreenSensor};

        let runner = RealCommandRunner {
            timeout: Some(std::time::Duration::from_secs(3)),
        };
        let run: CommandFn = Arc::new(move |program: &str, args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let out = runner.run(program, &args).ok()?;
            (out.status == Some(0)).then_some(out.stdout)
        });
        let mut loop_ = SensorLoop::new();
        let screen = HyprlandScreenSensor::new(run.clone());
        loop_.add(Box::new(if observe_pixels() {
            screen
        } else {
            screen.without_pixels()
        }));
        loop_.add(Box::new(HyprlandFocusSensor::new(run)));
        if let Some(mic) = voice.sensor() {
            loop_.add(Box::new(mic));
        }
        Some(Observer::new(loop_))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn live_observer(_voice: &VoiceConfig) -> Option<Observer> {
        None
    }

    /// Returns true if no goal is currently in flight.
    pub fn is_idle(&self) -> bool {
        match self.in_flight.lock() {
            Ok(g) => g.is_none(),
            Err(p) => p.into_inner().is_none(),
        }
    }

    /// Obtain a handle to inspect this runner's runtime status.
    pub fn handle(&self) -> GoalRunnerHandle {
        GoalRunnerHandle {
            in_flight: self.in_flight.clone(),
        }
    }

    /// Subscribe to `plan.goal` and run each goal through the Executive,
    /// publishing lifecycle events so the portal chat responds. A second,
    /// dedicated subscriber consumes the human-control events (`task.cancel` /
    /// `task.pause` / `task.approve`) so a control request reaches the live
    /// cycle even while the goal loop is blocked awaiting it.
    ///
    /// Both subscriptions are created *synchronously* before the tasks are
    /// spawned so a `plan.goal` / `task.*` published immediately after
    /// `start()` returns is never missed (broadcast receivers must exist
    /// before the send).
    pub fn start(self) -> GoalRunnerHandle {
        let handle = self.handle();
        let mut rx = self.bus.subscribe();
        let mut control_rx = self.bus.subscribe();
        let control_bus = self.bus.clone();
        let abort = self.abort.clone();
        let pause = self.pause.clone();
        let in_flight = self.in_flight.clone();
        let executive = self.executive.clone();

        // Control plane: consumes task.cancel / task.pause / task.approve
        // while a goal cycle runs. Matches the in-flight goal by its real run
        // id (from `goal.started`) — a control request for anything else is a
        // no-op (no fabricated state transition).
        tokio::spawn(async move {
            tracing::info!("goal runner control subscriber active");
            while let Some(ev) = crate::next_event(&mut control_rx).await {
                match ev.kind.as_str() {
                    "task.cancel" => {
                        let task_id = ev
                            .payload
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let matches = in_flight
                            .lock()
                            .unwrap()
                            .as_ref()
                            .is_some_and(|g| g.id == task_id);
                        if matches {
                            abort.store(true, std::sync::atomic::Ordering::SeqCst);
                            tracing::info!(task_id, "human cancelled the in-flight goal");
                        } else {
                            tracing::debug!(
                                task_id,
                                "task.cancel for unknown/not-in-flight goal — ignored"
                            );
                        }
                    }
                    "task.pause" => {
                        let task_id = ev
                            .payload
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let matches = in_flight
                            .lock()
                            .unwrap()
                            .as_ref()
                            .is_some_and(|g| g.id == task_id);
                        if matches {
                            pause.store(true, std::sync::atomic::Ordering::SeqCst);
                            tracing::info!(task_id, "human paused the in-flight goal");
                        } else {
                            tracing::debug!(
                                task_id,
                                "task.pause for unknown/not-in-flight goal — ignored"
                            );
                        }
                    }
                    "task.approve" => {
                        let task_id = ev
                            .payload
                            .get("task_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if task_id.is_empty() {
                            continue;
                        }
                        let mut exec = executive.lock().await;
                        match exec.approve_pending(&task_id) {
                            Ok(()) => {
                                let _ = control_bus
                                    .publish(Event::new(
                                        "goal.approved",
                                        "planner",
                                        json!({ "task_id": task_id }),
                                    ))
                                    .await;
                                tracing::info!(task_id, "held episode approved into memory");
                            }
                            Err(e) => tracing::debug!(
                                task_id,
                                "task.approve ignored (no held episode under this id): {e}"
                            ),
                        }
                    }
                    _ => {}
                }
            }
        });

        tokio::spawn(async move {
            tracing::info!("goal runner subscribed to plan.goal");
            while let Some(ev) = crate::next_event(&mut rx).await {
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
                let confirm = ev
                    .payload
                    .get("confirm")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
                let opts = AskOptions::from_payload(&ev.payload);
                self.run_goal(goal, session_id, confirm, opts).await;
            }
        });

        handle
    }

    /// Run one goal through the Executive and publish its lifecycle events,
    /// keyed by the session the goal arrived from. The session's prior turn is
    /// composed into the goal text the Executive plans on (per-session
    /// continuity), and the finished turn is recorded for the next goal in the
    /// same session.
    async fn run_goal(
        &self,
        goal: String,
        session_id: String,
        confirm: Option<String>,
        opts: AskOptions,
    ) {
        let (goal, masked_count) = friday_goal_execution::mask_secrets(&goal);
        let id = format!("goal-{}", uuid::Uuid::new_v4().simple());
        let egress_ctx = friday_intelligence::EgressContext {
            goal_id: Some(id.clone()),
            masked: Some(masked_count),
        };
        friday_intelligence::with_egress_context(
            egress_ctx,
            self.run_goal_inner(goal, id, masked_count, session_id, confirm, opts),
        )
        .await;
    }

    async fn run_goal_inner(
        &self,
        goal: String,
        id: String,
        masked_count: usize,
        session_id: String,
        confirm: Option<String>,
        opts: AskOptions,
    ) {
        let web = opts.web;
        // Lengths only: the question and any selected text (which can hold a
        // password or token) never go to the log.
        let asked = friday_goal_execution::planning::goal_segment(&goal)
            .chars()
            .count();
        tracing::info!(
            id = %id,
            session = %session_id,
            "goal received ({asked} chars asked, {} chars in all)",
            goal.chars().count()
        );

        // A fresh goal always starts with the control flags clear — a
        // previously cancelled/paused goal must not bleed into the next one.
        self.abort.store(false, std::sync::atomic::Ordering::SeqCst);
        self.pause.store(false, std::sync::atomic::Ordering::SeqCst);
        // Register as in-flight so the control subscriber can match
        // `task.cancel`/`task.pause` against this run's real id.
        *self.in_flight.lock().unwrap() = Some(InFlightGoal { id: id.clone() });
        {
            let mut g = self.confirmation_gate.lock().unwrap();
            g.confirm = confirm;
            g.pending = None;
        }

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
        let goal_start = std::time::Instant::now();

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

        let mut report = {
            let mut exec = self.executive.lock().await;
            // Keep focus deterministic: drop stale attention items so the fed
            // goal is always the next focus target.
            exec.attention_manager_mut().clear();
            exec.add_goal(composed.clone(), 0.9);
            exec.cycle(&self.workspace).await
        };

        // A confirmation is for this goal only: nothing that runs a tool later
        // (another goal, a background cycle) may reuse it.
        let pending_confirmation = {
            let mut gate = self.confirmation_gate.lock().unwrap();
            gate.confirm = None;
            gate.pending.take()
        };

        // Private mode: a web search would send the question out, so none is
        // made; the answer says so.
        let web_blocked = web && self.private_mode;
        let web_note = if web_blocked {
            "Private mode is on, so FRIDAY didn't search the web; this is from the local model alone."
        } else {
            "Web search failed, so this is from the model alone."
        };
        let (web_response, web_search_failed) = if web_blocked {
            (None, true)
        } else if web {
            let search_query = friday_goal_execution::planning::goal_segment(&goal);
            let (masked_query, _) = friday_goal_execution::mask_secrets(search_query);
            match self.web_searcher.search(&masked_query).await {
                Ok(resp) if !resp.results.is_empty() => (Some(resp), false),
                _ => (None, true),
            }
        } else {
            (None, false)
        };

        // The answer step: the cycle ran a tool and gathered evidence; the
        // model now writes the reply from them. Only for a goal that ran to
        // completion. If the model fails, the cycle's own result stands when
        // it is a real result (a command ran, files were listed); when it is
        // nothing to show (an empty memory lookup), the goal fails with why.
        let mut answer_failure: Option<String> = None;
        let mut answered_by_model = false;
        if pending_confirmation.is_none()
            && report.plan_executed
            && !report.cancelled
            && !report.paused
            && !report.recoverable
        {
            if let Some(answerer) = &self.answerer {
                // A cancel that lands while the model is writing stops the wait: the goal
                // ends as cancelled, not as completed with an answer nobody wanted.
                let abort = self.abort.clone();
                let cancelled = async move {
                    while !abort.load(std::sync::atomic::Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                };
                let composing = compose_answer(
                    answerer.as_ref(),
                    &composed,
                    &report,
                    &self.bus,
                    &id,
                    web_response.as_ref(),
                    &opts,
                );
                let composed_answer = tokio::select! {
                    r = composing => Some(r),
                    _ = cancelled => None,
                };
                match composed_answer {
                    None => report.cancelled = true,
                    Some(Ok(mut answer)) => {
                        if web {
                            if web_search_failed {
                                answer.text = format!("{web_note}\n\n{}", answer.text.trim());
                            } else if let Some(resp) = &web_response {
                                answer.text =
                                    append_sources(&answer.text, &resp.backend, &resp.results);
                            }
                        }
                        report.result = Some(answer);
                        answered_by_model = true;
                    }
                    Some(Err(e)) if !stands_without_the_model(report.result.as_ref()) => {
                        if let (true, false, Some(resp)) = (web, web_search_failed, &web_response) {
                            let mut text = format!("Web search results ({}):\n", resp.backend);
                            for (i, s) in resp.results.iter().enumerate() {
                                text.push_str(&format!("[{}] {}: {}\n", i + 1, s.title, s.snippet));
                            }
                            report.result = Some(GoalResult {
                                kind: GoalResultKind::Text,
                                text: append_sources(&text, &resp.backend, &resp.results),
                                structured: None,
                                tool: Some("web_search".to_string()),
                                success: true,
                            });
                        } else {
                            answer_failure = Some(crate::failures::plain_model_error(&e));
                        }
                    }
                    Some(Err(_)) => {}
                }
            } else if web {
                if web_search_failed {
                    report.result = Some(GoalResult {
                        kind: GoalResultKind::Text,
                        text: web_note.to_string(),
                        structured: None,
                        tool: Some("web_search".to_string()),
                        success: false,
                    });
                } else if let Some(resp) = &web_response {
                    let mut text = format!("Web search results ({}):\n", resp.backend);
                    for (i, s) in resp.results.iter().enumerate() {
                        text.push_str(&format!("[{}] {}: {}\n", i + 1, s.title, s.snippet));
                    }
                    report.result = Some(GoalResult {
                        kind: GoalResultKind::Text,
                        text: append_sources(&text, &resp.backend, &resp.results),
                        structured: None,
                        tool: Some("web_search".to_string()),
                        success: true,
                    });
                }
            }
        }

        // Record the finished turn for the next goal in this session.
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(ctx) = sessions.get_mut(&session_id) {
                ctx.last_goal = goal.clone();
                ctx.last_result = if report.cancelled {
                    "cancelled by human".to_string()
                } else if report.paused {
                    "paused by human".to_string()
                } else if let Some(nc) = &pending_confirmation {
                    format!("needs confirmation: {}", nc.action)
                } else {
                    report
                        .result
                        .as_ref()
                        .map(|r| truncate(&r.text, MAX_CONTEXT_CHARS))
                        .unwrap_or_default()
                };
                ctx.last_seen_ms = now_ms();
            }
        }
        evict_lru(&self.sessions, MAX_SESSIONS);

        if report.cancelled {
            // The human stopped this goal: publish the terminal `goal.cancelled`
            // — the actual state transition, never a silent ack. The cycle
            // already skipped remaining actions and recorded no episode.
            let _ = self
                .bus
                .publish(Event::new(
                    "goal.cancelled",
                    "planner",
                    json!({
                        "goal": goal,
                        "id": id,
                        "session_id": session_id,
                        "session_turn": next_turn,
                        "reason": "cancelled_by_human",
                        "phases": report.phases_completed.len(),
                    }),
                ))
                .await;
        } else if report.paused {
            let _ = self
                .bus
                .publish(Event::new(
                    "goal.paused",
                    "planner",
                    json!({
                        "goal": goal,
                        "id": id,
                        "session_id": session_id,
                        "session_turn": next_turn,
                        "reason": "paused_by_human",
                        "phases": report.phases_completed.len(),
                    }),
                ))
                .await;
        } else if let Some(pending) = pending_confirmation {
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
                        "error": format!("Confirm to run: {}", pending.action),
                        "needs_confirmation": {
                            "tool": pending.tool,
                            "action": pending.action,
                        },
                        "phases": report.phases_completed.len(),
                    }),
                ))
                .await;
        } else if report.recoverable {
            // S5-B: the goal's action exhausted its bounded recovery — a
            // genuine, retryable failure, not a phantom completion. Publish the
            // recovery evidence (why it failed and the last observed state) so
            // a follow-up goal can re-enter from evidence instead of cold.
            let reason = report
                .failed_reason
                .clone()
                .unwrap_or_else(|| "action failed after bounded recovery".to_string());
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
                        "recoverable": true,
                        "failed_reason": report.failed_reason,
                        "recovery_attempts": report.recovery_attempts,
                        "last_observed_state": report.last_observed_state,
                    }),
                ))
                .await;
        } else if let Some(reason) = &answer_failure {
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
        } else if report.plan_executed {
            // The fabric is called for the investigation and the answer step;
            // its last routing decision is THIS goal's answer step: `served` is
            // Some((provider, model)) when the model answered, None when it
            // errored and the heuristic planned instead.
            let served = self
                .fabric
                .as_ref()
                .and_then(|f| f.last_decision())
                .and_then(|d| d.served);
            let elapsed_ms = goal_start.elapsed().as_millis() as u64;
            let served_by = if answered_by_model {
                served
                    .as_ref()
                    .map(|(provider, model)| crate::shell_state::ServedBy {
                        provider: provider.clone(),
                        model: model.clone(),
                        ms: elapsed_ms,
                    })
            } else {
                None
            };
            let mut payload = completed_payload(
                &goal,
                &id,
                &report,
                served.as_ref(),
                served_by,
                masked_count,
            );
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
            let mut payload = json!({
                "goal": goal,
                "id": id,
                "session_id": session_id,
                "session_turn": next_turn,
                "error": reason,
            });
            if masked_count > 0 {
                payload["masked"] = json!(masked_count);
            }
            let _ = self
                .bus
                .publish(Event::new("goal.failed", "planner", payload))
                .await;
        }
        let terminal = if report.cancelled {
            "goal.cancelled"
        } else if report.paused {
            "goal.paused"
        } else if report.recoverable {
            "goal.recoverable"
        } else if answer_failure.is_some() {
            "goal.failed"
        } else if report.plan_executed {
            "goal.completed"
        } else {
            "goal.failed"
        };
        tracing::info!(id = %id, session = %session_id, "goal terminal: {terminal}");

        // The run is over — clear in_flight only AFTER the terminal event is
        // published to the bus, so the upgrade watcher cannot exit between them.
        *self.in_flight.lock().unwrap() = None;
    }
}

/// How long the answer step waits for the model before keeping the tool's
/// own result (friday-ctl --wait gives the whole goal 180 s).
const ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// Whether the cycle's own result is worth showing when the model gave no
/// answer: a tool that succeeded with something to say is; a failed tool, an
/// empty memory lookup or nothing isn't.
fn stands_without_the_model(result: Option<&GoalResult>) -> bool {
    match result {
        None => false,
        Some(r) => {
            let text = r.text.trim();
            r.success
                && !text.is_empty()
                && !(r.tool.as_deref() == Some("memory") && text == "no memory matches yet")
        }
    }
}

/// How often a streaming answer is published as `goal.progress` (each one is
/// a state.json write, so not on every token).
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_millis(150);

/// Publish the latest answer text as `goal.progress` {id, partial} whenever
/// it changed, at most every [`PROGRESS_EVERY`], until the sender is gone.
async fn forward_progress(
    mut rx: tokio::sync::watch::Receiver<String>,
    bus: EventBus,
    goal_id: String,
) {
    let mut sent = String::new();
    loop {
        let open = rx.changed().await.is_ok();
        let text = rx.borrow_and_update().clone();
        if !text.trim().is_empty() && text != sent {
            let _ = bus
                .publish(Event::new(
                    "goal.progress",
                    "planner",
                    json!({ "id": goal_id, "partial": text }),
                ))
                .await;
            sent = text;
        }
        if !open {
            return;
        }
        tokio::time::sleep(PROGRESS_EVERY).await;
    }
}
/// How much tool output / evidence goes into the answer prompt.
const ANSWER_SOURCE_CHARS: usize = 6000;

/// The prompt for the answer step: the user's request (with any session
/// context and recalled memory the runner composed), then what FRIDAY did and
/// found. The model answers from those; it is told not to invent beyond them
/// for facts about the user's machine.
fn answer_prompt(
    request: &str,
    report: &ExecutiveReport,
    web_response: Option<&SearchResponse>,
) -> String {
    let mut p = String::from(
        "You are FRIDAY, a desktop assistant. Answer the user's request directly and \
         concisely, in plain text. If the request includes text to work on (selected or \
         copied text), work on that text. Use what FRIDAY did and found below when it is \
         relevant; for facts about the user's own machine, files or history, rely only \
         on it, and say so when it doesn't contain the answer. If an action was taken, \
         say what happened.\n\n",
    );
    if let Some(resp) = web_response {
        if !resp.results.is_empty() {
            let label = if resp.backend.is_empty() {
                "Sources from the web:\n".to_string()
            } else {
                format!("Sources from the web ({}):\n", resp.backend)
            };
            p.push_str(&label);
            for (i, s) in resp.results.iter().enumerate() {
                p.push_str(&format!(
                    "[{}] {} ({}): {}\n",
                    i + 1,
                    s.title,
                    s.url,
                    s.snippet.trim()
                ));
            }
            if resp.backend.eq_ignore_ascii_case("wikipedia") {
                p.push_str("\nThese are encyclopedia results that may not cover very recent events. Answer the request using the sources above when relevant. Cite sources as [1], [2], etc. If the sources do not contain enough information to answer the question, say so plainly.\n\n");
            } else {
                p.push_str("\nAnswer the request using the sources above when relevant. Cite sources as [1], [2], etc. If the sources do not contain enough information to answer the question, say so plainly.\n\n");
            }
        }
    }
    p.push_str("Request:\n");
    p.push_str(request.trim());
    p.push('\n');
    if let Some(r) = &report.result {
        if !r.text.trim().is_empty() {
            let tool = r.tool.as_deref().unwrap_or("tool");
            let label = if r.success { "output" } else { "error" };
            p.push_str(&format!(
                "\nWhat FRIDAY did ({tool}, {label}):\n{}\n",
                // Masked before it is cut, so a key can't be left half showing.
                truncate(
                    &friday_goal_execution::mask_secrets(&r.text).0,
                    ANSWER_SOURCE_CHARS
                )
            ));
        }
    }
    if let Some(ev) = &report.evidence {
        let mut block = String::new();
        for item in &ev.set.items {
            block.push_str(&format!(
                "- {} ({}): {}\n",
                item.title,
                item.url,
                item.content.trim()
            ));
        }
        if !block.is_empty() {
            p.push_str(&format!(
                "\nEvidence:\n{}",
                truncate(
                    &friday_goal_execution::mask_secrets(&block).0,
                    ANSWER_SOURCE_CHARS
                )
            ));
        }
    }
    p.push_str("\nAnswer:\n");
    p
}

/// How a goal was asked, beyond its words (`plan.goal`'s optional fields).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskOptions {
    /// Search the web and answer from the results, with sources.
    pub web: bool,
    /// Ask again for a better answer: a model ranked for quality, told to
    /// take more care, and not `avoid`.
    pub thorough: bool,
    /// The `(provider, model)` that gave the answer being retried.
    pub avoid: Option<(String, String)>,
}

impl AskOptions {
    pub fn from_payload(payload: &serde_json::Value) -> Self {
        let flag = |k: &str| payload.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let avoid = payload.get("avoid").and_then(|a| {
            let field = |k: &str| {
                a.get(k)
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.trim().is_empty())
                    .map(str::to_string)
            };
            Some((field("provider")?, field("model")?))
        });
        Self {
            web: flag("web"),
            thorough: flag("thorough"),
            avoid,
        }
    }
}

/// Put first in a thorough answer's prompt.
const THOROUGH_NOTE: &str = "The person asked again for a better answer than the last one. \
Take more care this time: think the question through, check facts, numbers and steps, \
and cover what matters, while staying as short as a complete answer allows.\n\n";

/// The answer step: ask the model to reply to `request` from what the cycle
/// produced. Returns the reply as a `Text` result (the tool's own output is
/// kept in `structured.source`), or `None` when the model errors, times out or
/// says nothing, so the caller keeps the cycle's result.
async fn compose_answer(
    client: &dyn LlmStrategyClient,
    request: &str,
    report: &ExecutiveReport,
    bus: &EventBus,
    goal_id: &str,
    web_response: Option<&SearchResponse>,
    opts: &AskOptions,
) -> Result<GoalResult, String> {
    let mut prompt = answer_prompt(request, report, web_response);
    if opts.thorough {
        prompt.insert_str(0, THOROUGH_NOTE);
    }
    // The answer streams: the text so far goes out as `goal.progress` events
    // (at most every PROGRESS_EVERY), and the forwarder is drained before
    // returning so no progress can land after the goal's terminal event.
    let (tx, rx) = tokio::sync::watch::channel(String::new());
    let forwarder = tokio::spawn(forward_progress(rx, bus.clone(), goal_id.to_string()));
    let progress = move |text: &str| {
        let _ = tx.send(text.to_string());
    };
    let outcome = if opts.thorough {
        let avoid = opts.avoid.as_ref().map(|(p, m)| (p.as_str(), m.as_str()));
        let call = client.complete_thorough_with_progress(&prompt, avoid, &progress);
        tokio::time::timeout(ANSWER_TIMEOUT, call).await
    } else {
        let call = client.complete_with_progress(&prompt, &progress);
        tokio::time::timeout(ANSWER_TIMEOUT, call).await
    };
    drop(progress);
    let _ = forwarder.await;
    let reply = match outcome {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            tracing::warn!("answer step: model error: {e}");
            return Err(e);
        }
        Err(_) => {
            tracing::warn!("answer step: model timed out after {ANSWER_TIMEOUT:?}");
            return Err(format!("request timed out after {ANSWER_TIMEOUT:?}"));
        }
    };
    let text = reply.trim();
    if text.is_empty() {
        return Err("the model returned an empty answer".to_string());
    }
    let source = report.result.as_ref().map(|r| {
        json!({ "tool": r.tool, "success": r.success, "text": truncate(&r.text, ANSWER_SOURCE_CHARS) })
    });
    Ok(GoalResult {
        kind: GoalResultKind::Text,
        text: text.to_string(),
        structured: source.map(|s| json!({ "source": s })),
        tool: Some("answer".to_string()),
        success: true,
    })
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
    served_by: Option<crate::shell_state::ServedBy>,
    masked: usize,
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
        // S5 recovery evidence: whether the goal's action recovered on a bounded
        // retry after re-observation, and how many execute attempts ran
        // (1 = no recovery was needed).
        "recovered": report.recovered,
        "recovery_attempts": report.recovery_attempts,
        // S6 perception evidence: how many real sensor observations the
        // Observe phase folded into the world state this run (0 = no observer
        // attached or nothing observable fired).
        "observations_folded": report.observations_folded,
        // S7 learning receipts: how many policies the cycle derived (consolidated
        // knowledge in semantic memory) and how many qualified for this goal's
        // planning via the relevance gate (0 = no learned policy matched yet).
        "policies_derived": report.policies_derived,
        "policies_applied": report.policies_applied,
    });
    // A held episode (admission Pending, e.g. sensitive content) was NOT
    // stored — surface its id so the portal can offer a `task.approve` button
    // for this exact episode (S3).
    if matches!(report.admission_verdict, Some(AdmissionVerdict::Pending(_))) {
        if let Some(held_id) = &report.held_episode_id {
            payload["pending_approval"] = json!(true);
            payload["pending_approval_id"] = json!(held_id);
        }
    }
    if let Some((provider, model)) = served {
        payload["provider"] = json!(provider);
        payload["model"] = json!(model);
    }
    if let Some(sb) = served_by {
        payload["served_by"] = json!({
            "provider": sb.provider,
            "model": sb.model,
            "ms": sb.ms,
        });
    }
    if masked > 0 {
        payload["masked"] = json!(masked);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_search::SearchResult;

    /// A tiny canned search-results page in the endpoint's HTML shape.
    const MOCK_SEARCH_BODY: &str = r#"<html><body>
        <a href="https://docs.rs/tokio/latest/tokio/">Tokio documentation</a>
        <a href="https://rust-lang.github.io/async-book/">The Rust Async Book</a>
    </body></html>"#;

    /// Spin up a hermetic mock search endpoint (no network). When `delay_ms` >
    /// 0 each response is parked that long, so a web goal is still mid-
    /// execution when a test's control command lands — deterministic control
    /// tests without timing races. Serves up to `max` requests, then exits
    /// after a short idle window.
    /// How long a test waits for the next event on a loaded CI runner (the
    /// Windows one is slow); a healthy run is well under a second.
    const SLOW_RUNNER_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

    fn mock_search_endpoint(delay_ms: u64, max: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking mock server");
        let addr = listener.local_addr().expect("mock server address");
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let mut served = 0usize;
            let mut last_activity = std::time::Instant::now();
            while std::time::Instant::now() < deadline && served < max {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if last_activity.elapsed() > std::time::Duration::from_secs(2) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
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
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    MOCK_SEARCH_BODY.len(),
                    MOCK_SEARCH_BODY
                );
                let _ = stream.write_all(resp.as_bytes());
                served += 1;
            }
        });
        (format!("http://{addr}/"), handle)
    }

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
            json!({ "goal": "git status", "confirm": "git status" }),
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
                    assert_eq!(ev.payload["goal"], "git status");
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
    /// A scripted model for the answer step: replies with `reply` (or fails)
    /// and records every prompt it was given.
    struct ScriptedAnswerer {
        reply: Result<String, String>,
        prompts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl LlmStrategyClient for ScriptedAnswerer {
        async fn complete(&self, prompt: &str) -> Result<String, String> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            self.reply.clone()
        }
    }

    /// Records which call the answer step made: a normal answer, or a
    /// thorough one and the model it was told to avoid.
    /// (which call, the model to avoid, the prompt)
    type AnswerCall = (String, Option<(String, String)>, String);

    #[derive(Default)]
    struct RetryAnswerer {
        calls: Mutex<Vec<AnswerCall>>,
    }

    #[async_trait]
    impl LlmStrategyClient for RetryAnswerer {
        async fn complete(&self, prompt: &str) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(("normal".into(), None, prompt.to_string()));
            Ok("quick answer".to_string())
        }
        async fn complete_thorough_with_progress(
            &self,
            prompt: &str,
            avoid: Option<(&str, &str)>,
            progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            self.calls.lock().unwrap().push((
                "thorough".into(),
                avoid.map(|(p, m)| (p.to_string(), m.to_string())),
                prompt.to_string(),
            ));
            progress("careful answer");
            Ok("careful answer".to_string())
        }
    }

    async fn run_payload(
        payload: serde_json::Value,
        answerer: Arc<RetryAnswerer>,
    ) -> serde_json::Value {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone()).with_answerer(answerer);
        runner.start();
        bus.publish(Event::new("plan.goal", "mobile", payload))
            .await
            .expect("publish");
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("a terminal event in time")
                .expect("bus alive");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                return ev.payload;
            }
        }
        panic!("no terminal event");
    }

    /// "Try a better model": a thorough goal takes the thorough call, with
    /// the model to avoid and the take-more-care note; a plain goal doesn't.
    #[tokio::test]
    async fn thorough_goal_asks_for_a_careful_answer_from_another_model() {
        let answerer = Arc::new(RetryAnswerer::default());
        let payload = run_payload(
            json!({
                "goal": "What does the Rust error E0308 mean?",
                "thorough": true,
                "avoid": { "provider": "opencode", "model": "big-pickle" },
            }),
            answerer.clone(),
        )
        .await;
        assert_eq!(payload["result"]["text"], "careful answer", "{payload}");
        {
            let calls = answerer.calls.lock().unwrap();
            assert_eq!(calls.len(), 1, "{calls:?}");
            assert_eq!(calls[0].0, "thorough");
            assert_eq!(
                calls[0].1,
                Some(("opencode".to_string(), "big-pickle".to_string()))
            );
            assert!(calls[0].2.starts_with(THOROUGH_NOTE), "{}", calls[0].2);
        }

        let plain = Arc::new(RetryAnswerer::default());
        let payload = run_payload(
            json!({ "goal": "What does the Rust error E0308 mean?" }),
            plain.clone(),
        )
        .await;
        assert_eq!(payload["result"]["text"], "quick answer", "{payload}");
        let calls = plain.calls.lock().unwrap();
        assert_eq!(calls[0].0, "normal");
        assert!(!calls[0].2.contains(THOROUGH_NOTE));
    }

    #[test]
    fn ask_options_read_from_the_goal_payload() {
        assert_eq!(
            AskOptions::from_payload(&json!({ "goal": "x" })),
            AskOptions::default()
        );
        let o = AskOptions::from_payload(&json!({
            "web": true,
            "thorough": true,
            "avoid": { "provider": "kilo", "model": "a/b" },
        }));
        assert!(o.web && o.thorough);
        assert_eq!(o.avoid, Some(("kilo".to_string(), "a/b".to_string())));
        // Half an `avoid` is no `avoid`.
        let o = AskOptions::from_payload(&json!({ "avoid": { "provider": "kilo" } }));
        assert_eq!(o.avoid, None);
    }

    /// Streams its reply in words, with pauses longer than the progress
    /// throttle so each step is published.
    struct StreamingAnswerer {
        words: Vec<&'static str>,
    }

    #[async_trait]
    impl LlmStrategyClient for StreamingAnswerer {
        async fn complete(&self, _prompt: &str) -> Result<String, String> {
            Ok(self.words.concat())
        }
        async fn complete_with_progress(
            &self,
            _prompt: &str,
            progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
        ) -> Result<String, String> {
            let mut text = String::new();
            for w in &self.words {
                text.push_str(w);
                progress(&text);
                tokio::time::sleep(PROGRESS_EVERY * 2).await;
            }
            Ok(text)
        }
    }

    /// The answer streams as `goal.progress` events, growing, for this goal,
    /// all before its `goal.completed`.
    #[tokio::test]
    async fn answer_streams_as_progress_before_completion() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(StreamingAnswerer {
            words: vec!["E0308 ", "means ", "mismatched types."],
        });
        GoalRunner::new(bus.clone()).with_answerer(answerer).start();
        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "What is E0308?" }),
        ))
        .await
        .expect("publish");
        let (mut id, mut partials, mut done) = (String::new(), Vec::new(), None);
        for _ in 0..200 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("an event in time")
                .expect("bus open");
            match ev.kind.as_str() {
                "goal.started" => id = ev.payload["id"].as_str().unwrap_or("").to_string(),
                "goal.progress" => {
                    assert!(done.is_none(), "progress after completion: {}", ev.payload);
                    assert_eq!(ev.payload["id"], id.as_str());
                    partials.push(ev.payload["partial"].as_str().unwrap().to_string());
                }
                "goal.completed" => {
                    done = Some(ev.payload.clone());
                    break;
                }
                _ => {}
            }
        }
        let done = done.expect("the goal completed");
        assert_eq!(done["result"]["text"], "E0308 means mismatched types.");
        assert_eq!(
            partials,
            ["E0308 ", "E0308 means ", "E0308 means mismatched types."],
            "each step of the answer, growing"
        );
    }

    /// Run one goal on a hermetic runner with the given answerer and return
    /// the terminal event's payload.
    async fn run_with_answerer(goal: &str, answerer: Arc<ScriptedAnswerer>) -> serde_json::Value {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone()).with_answerer(answerer);
        runner.start();
        bus.publish(Event::new("plan.goal", "mobile", json!({ "goal": goal })))
            .await
            .expect("publish");
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("a terminal event in time")
                .expect("bus alive");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                assert_eq!(ev.kind, "goal.completed", "{:?}", ev.payload);
                return ev.payload;
            }
        }
        panic!("no terminal event");
    }

    /// The answer step: a question gets the model's reply as its result, and
    /// the tool's own output rides along as the source.
    #[tokio::test]
    async fn answer_step_makes_the_model_reply_the_result() {
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("  E0308 means mismatched types.  ".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let payload =
            run_with_answerer("What does the Rust error E0308 mean?", answerer.clone()).await;
        assert_eq!(payload["result"]["kind"], "text");
        assert_eq!(payload["result"]["text"], "E0308 means mismatched types.");
        assert_eq!(payload["result"]["tool"], "answer");
        assert!(
            payload["result"]["structured"]["source"].is_object(),
            "{payload}"
        );
        let prompts = answerer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1, "one answer call per goal");
        assert!(
            prompts[0].contains("Request:\nWhat does the Rust error E0308 mean?"),
            "{}",
            prompts[0]
        );
    }

    /// A model that takes its time, so a cancel can land while it writes.
    struct SlowAnswerer;

    #[async_trait]
    impl LlmStrategyClient for SlowAnswerer {
        async fn complete(&self, _prompt: &str) -> Result<String, String> {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok("too late".to_string())
        }
    }

    /// A `task.cancel` that arrives while the model is answering ends the goal as
    /// `goal.cancelled`, quickly, and never as `goal.completed`.
    #[tokio::test]
    async fn task_cancel_while_the_model_answers_cancels_the_goal() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone()).with_answerer(Arc::new(SlowAnswerer));
        runner.start();
        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "What does the Rust error E0308 mean?" }),
        ))
        .await
        .expect("publish");

        let mut cancel_sent = false;
        let started = std::time::Instant::now();
        loop {
            let ev = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
                .await
                .expect("a terminal event before the model would have answered")
                .expect("bus alive");
            match ev.kind.as_str() {
                "goal.started" => {
                    let id = ev.payload["id"].as_str().expect("goal id").to_string();
                    // The plan runs in a moment; the model is then the slow part.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    bus.publish(Event::new(
                        "task.cancel",
                        "mobile",
                        json!({ "task_id": id }),
                    ))
                    .await
                    .expect("publish");
                    cancel_sent = true;
                }
                "goal.cancelled" => break,
                "goal.completed" | "goal.failed" => {
                    panic!("a cancelled goal must not end as {}", ev.kind)
                }
                _ => {}
            }
        }
        assert!(cancel_sent);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn goal_containing_secret_never_reaches_answerer_unmasked() {
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("I can explain this code safely.".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let raw_key = "sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE";
        let goal = format!("Explain this code\n\nUse this text as context:\n\"\"\"\nlet key = \"{raw_key}\";\n\"\"\"");
        let payload = run_with_answerer(&goal, answerer.clone()).await;
        assert_eq!(payload["masked"], 1);

        let prompts = answerer.prompts.lock().unwrap();
        assert!(
            !prompts.is_empty(),
            "answerer must have received at least one prompt"
        );
        for p in prompts.iter() {
            assert!(
                !p.contains(raw_key),
                "prompt must NEVER contain the raw secret: {p}"
            );
            assert!(
                p.contains("[secret masked]"),
                "prompt must contain the masked placeholder: {p}"
            );
        }
    }

    struct FakeWebSearcher {
        calls: std::sync::atomic::AtomicUsize,
        results: Result<SearchResponse, String>,
    }

    #[async_trait]
    impl WebSearcher for FakeWebSearcher {
        async fn search(&self, _query: &str) -> Result<SearchResponse, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.results.clone()
        }
    }

    #[tokio::test]
    async fn goal_without_web_never_calls_search() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("4".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let searcher = Arc::new(FakeWebSearcher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            results: Ok(SearchResponse {
                backend: "Wikipedia".to_string(),
                results: vec![SearchResult {
                    title: "T".to_string(),
                    url: "https://example.com".to_string(),
                    snippet: "S".to_string(),
                }],
            }),
        });
        GoalRunner::new(bus.clone())
            .with_answerer(answerer)
            .with_web_searcher(searcher.clone())
            .start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "what is 2+2" }),
        ))
        .await
        .expect("publish");

        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("event")
                .expect("bus");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                break;
            }
        }

        assert_eq!(
            searcher.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "must not search without web"
        );
    }

    #[tokio::test]
    async fn goal_with_web_calls_search_and_appends_sources() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("The latest version is 1.100.0 according to [1].".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let searcher = Arc::new(FakeWebSearcher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            results: Ok(SearchResponse {
                backend: "DuckDuckGo".to_string(),
                results: vec![
                    SearchResult {
                        title: "Rust Releases".to_string(),
                        url: "https://doc.rust-lang.org/stable/releases.html".to_string(),
                        snippet: "Rust 1.100.0 is released".to_string(),
                    },
                    SearchResult {
                        title: "Rust Blog".to_string(),
                        url: "https://blog.rust-lang.org/".to_string(),
                        snippet: "Announcement".to_string(),
                    },
                ],
            }),
        });
        GoalRunner::new(bus.clone())
            .with_answerer(answerer.clone())
            .with_web_searcher(searcher.clone())
            .start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "what is the latest rust", "web": true }),
        ))
        .await
        .expect("publish");

        let mut done = None;
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("event")
                .expect("bus");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                done = Some(ev.payload);
                break;
            }
        }

        let done = done.expect("completed");
        assert_eq!(
            searcher.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "must call search when web is true"
        );
        let result_text = done["result"]["text"].as_str().expect("result text");
        assert!(result_text.contains("The latest version is 1.100.0 according to [1]."));
        assert!(result_text.contains(
            "Sources (DuckDuckGo):\n[1] Rust Releases — https://doc.rust-lang.org/stable/releases.html"
        ));
        // Source [2] was not cited
        assert!(!result_text.contains("[2]"));

        // Verify that sources were in the prompt
        let prompts = answerer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Sources from the web (DuckDuckGo):"));
        assert!(prompts[0].contains("[1] Rust Releases (https://doc.rust-lang.org/stable/releases.html): Rust 1.100.0 is released"));
    }

    #[tokio::test]
    async fn goal_with_wikipedia_web_search_carries_encyclopedia_instruction() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("Rust was created by Graydon Hoare [1].".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let searcher = Arc::new(FakeWebSearcher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            results: Ok(SearchResponse {
                backend: "Wikipedia".to_string(),
                results: vec![SearchResult {
                    title: "Rust (programming language)".to_string(),
                    url: "https://en.wikipedia.org/wiki/Rust_(programming_language)".to_string(),
                    snippet: "Rust is a general-purpose programming language.".to_string(),
                }],
            }),
        });
        GoalRunner::new(bus.clone())
            .with_answerer(answerer.clone())
            .with_web_searcher(searcher.clone())
            .start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "who created rust", "web": true }),
        ))
        .await
        .expect("publish");

        let mut done = None;
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("event")
                .expect("bus");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                done = Some(ev.payload);
                break;
            }
        }

        let done = done.expect("completed");
        let result_text = done["result"]["text"].as_str().expect("result text");
        assert!(result_text.contains("Sources (Wikipedia):"));

        let prompts = answerer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Sources from the web (Wikipedia):"));
        assert!(prompts[0]
            .contains("These are encyclopedia results that may not cover very recent events."));
    }

    #[tokio::test]
    async fn web_search_failure_fallback_says_model_alone() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("Rust is a programming language.".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let searcher = Arc::new(FakeWebSearcher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            results: Err("network error".to_string()),
        });
        GoalRunner::new(bus.clone())
            .with_answerer(answerer.clone())
            .with_web_searcher(searcher.clone())
            .start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "what is rust", "web": true }),
        ))
        .await
        .expect("publish");

        let mut done = None;
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("event")
                .expect("bus");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                done = Some(ev.payload);
                break;
            }
        }

        let done = done.expect("completed");
        assert_eq!(searcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let result_text = done["result"]["text"].as_str().expect("result text");
        assert!(result_text.contains("Web search failed, so this is from the model alone."));
        assert!(result_text.contains("Rust is a programming language."));

        // Prompt must NOT contain Sources from the web
        let prompts = answerer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(!prompts[0].contains("Sources from the web:"));
    }

    #[tokio::test]
    async fn private_mode_never_searches_the_web() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Ok("From the local model.".to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let searcher = Arc::new(FakeWebSearcher {
            calls: std::sync::atomic::AtomicUsize::new(0),
            results: Ok(SearchResponse {
                backend: "Wikipedia".to_string(),
                results: vec![SearchResult {
                    title: "T".to_string(),
                    url: "https://example.com".to_string(),
                    snippet: "S".to_string(),
                }],
            }),
        });
        GoalRunner::new(bus.clone())
            .with_answerer(answerer)
            .with_web_searcher(searcher.clone())
            .with_private_mode(true)
            .start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "what is rust", "web": true }),
        ))
        .await
        .expect("publish");

        let mut done = None;
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("event")
                .expect("bus");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                done = Some(ev.payload);
                break;
            }
        }
        let done = done.expect("completed");
        assert_eq!(searcher.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let text = done["result"]["text"].as_str().expect("result text");
        assert!(
            text.contains("Private mode is on, so FRIDAY didn't search the web"),
            "{text}"
        );
    }

    /// Run one goal and return its terminal event's (kind, payload).
    async fn terminal_with_answerer(
        goal: &str,
        answerer: Arc<ScriptedAnswerer>,
    ) -> (String, serde_json::Value) {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        GoalRunner::new(bus.clone()).with_answerer(answerer).start();
        bus.publish(Event::new("plan.goal", "mobile", json!({ "goal": goal })))
            .await
            .expect("publish");
        for _ in 0..40 {
            let ev = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .expect("a terminal event in time")
                .expect("bus alive");
            if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                return (ev.kind, ev.payload);
            }
        }
        panic!("no terminal event");
    }

    /// The model failed and the cycle has nothing to show (here its tool
    /// failed): the goal fails, saying why in plain words, instead of
    /// "completing" with a tool error or an empty lookup.
    #[tokio::test]
    async fn model_failure_with_nothing_to_show_fails_plainly() {
        let answerer = Arc::new(ScriptedAnswerer {
            reply: Err("HTTP request failed: error sending request for url \
                (http://127.0.0.1:9/v1/chat/completions): Connection refused"
                .to_string()),
            prompts: Mutex::new(Vec::new()),
        });
        let (kind, payload) =
            terminal_with_answerer("What does the Rust error E0308 mean?", answerer.clone()).await;
        assert_eq!(kind, "goal.failed", "{payload}");
        let error = payload["error"].as_str().unwrap_or_default();
        assert!(
            error.starts_with("Couldn't reach your AI provider"),
            "{error}"
        );
        assert_eq!(answerer.prompts.lock().unwrap().len(), 1);
    }

    /// A real tool result still stands when the model fails; nothing else does.
    #[test]
    fn only_a_real_tool_result_stands_without_the_model() {
        let ok = |tool: &str, text: &str| {
            GoalResult::from_tool_result(&ToolResult::success(text)).with_tool(tool)
        };
        assert!(stands_without_the_model(Some(&ok(
            "terminal",
            "42 tests passed"
        ))));
        assert!(!stands_without_the_model(Some(&ok(
            "memory",
            "no memory matches yet"
        ))));
        assert!(!stands_without_the_model(Some(&ok("terminal", "   "))));
        let failed = GoalResult::from_tool_result(&ToolResult::error("no tool for capability"));
        assert!(!stands_without_the_model(Some(&failed)));
        assert!(!stands_without_the_model(None));
    }

    /// A key a tool printed (a file read, a command's output) never goes to the model.
    #[test]
    fn answer_prompt_masks_secrets_in_tool_output() {
        let key = "sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE";
        let report = ExecutiveReport {
            result: Some(
                GoalResult::from_tool_result(&ToolResult::success(format!("TOKEN=\"{key}\"")))
                    .with_tool("filesystem"),
            ),
            ..Default::default()
        };
        let p = answer_prompt("what is in this file", &report, None);
        assert!(!p.contains(key), "{p}");
        assert!(p.contains("[secret masked]"), "{p}");
    }

    /// The prompt carries the request, what the tool produced, and evidence.
    #[test]
    fn answer_prompt_carries_request_tool_output_and_evidence() {
        let report = ExecutiveReport {
            result: Some(
                GoalResult::from_tool_result(&ToolResult::success("42 tests passed"))
                    .with_tool("terminal"),
            ),
            ..Default::default()
        };
        let p = answer_prompt(
            "summarise the test run\n\n[memory]\nlast run failed",
            &report,
            None,
        );
        assert!(p.contains("Request:\nsummarise the test run"));
        assert!(
            p.contains("[memory]\nlast run failed"),
            "session/memory context is kept"
        );
        assert!(p.contains("What FRIDAY did (terminal, output):\n42 tests passed"));
        assert!(p.trim_end().ends_with("Answer:"));
    }

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
            json!({ "goal": "git status", "confirm": "git status" }),
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

        let report = ExecutiveReport {
            plan_executed: true,
            evidence_trail: EvidenceTrail {
                goal: "git status".into(),
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
            },
            ..ExecutiveReport::default()
        };

        // The fabric served a model → provider/model + strategy=llm.
        let served = (
            "openrouter".to_string(),
            "openrouter/nvidia/nemotron:free".to_string(),
        );
        let p = completed_payload("g", "id-1", &report, Some(&served), None, 0);
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
        let p = completed_payload("g", "id-2", &report, None, None, 0);
        assert_eq!(p["strategy"], "heuristic");
        assert!(p.get("provider").is_none());
        assert!(p.get("model").is_none());
        // The channels are still the evidence-gated plan's channels.
        assert_eq!(p["channels"][0]["channel"], "git");
    }

    #[test]
    fn completed_payload_carries_served_by() {
        let report = ExecutiveReport {
            plan_executed: true,
            ..ExecutiveReport::default()
        };
        let served = (
            "opencode-zen".to_string(),
            "inclusionai/ling-3.0-flash:free".to_string(),
        );
        let sb = crate::shell_state::ServedBy {
            provider: "opencode-zen".to_string(),
            model: "inclusionai/ling-3.0-flash:free".to_string(),
            ms: 2300,
        };
        let p = completed_payload("g", "id-sb", &report, Some(&served), Some(sb), 0);
        assert_eq!(p["served_by"]["provider"], "opencode-zen");
        assert_eq!(p["served_by"]["model"], "inclusionai/ling-3.0-flash:free");
        assert_eq!(p["served_by"]["ms"], 2300);

        // When absent, served_by is not in the payload
        let p_none = completed_payload("g", "id-nosb", &report, Some(&served), None, 0);
        assert!(p_none.get("served_by").is_none());
    }

    #[test]
    fn completed_payload_carries_masked_count() {
        let report = ExecutiveReport {
            plan_executed: true,
            ..ExecutiveReport::default()
        };
        let p = completed_payload("g", "id-masked", &report, None, None, 3);
        assert_eq!(p["masked"], 3);

        let p_zero = completed_payload("g", "id-nomasked", &report, None, None, 0);
        assert!(p_zero.get("masked").is_none());
    }

    /// The completed payload contract carries the goal's canonical result
    /// verbatim: the kind, the renderable text (the actual execution output),
    /// and the producing tool — the exact JSON the frontend `parseResult`
    /// reads across the WebSocket boundary.
    #[test]
    fn completed_payload_carries_the_goal_result() {
        use friday_goal_execution::{GoalResult, ToolResult};

        let report = ExecutiveReport {
            plan_executed: true,
            result: Some(
                GoalResult::from_tool_result(&ToolResult::success(
                    "Here are the files in the project:\n- Cargo.toml\n- frontend/\n- services/",
                ))
                .with_tool("filesystem"),
            ),
            ..ExecutiveReport::default()
        };

        let p = completed_payload(
            "List the files in this project",
            "id-3",
            &report,
            None,
            None,
            0,
        );
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
    fn a_file_without_an_extension_is_found_by_its_name() {
        let dir = std::env::temp_dir().join(format!("friday-bare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Dockerfile"), "FROM scratch").unwrap();
        let cwd = dir.to_str().unwrap();
        assert_eq!(
            bare_file_token("Read the Dockerfile.", cwd),
            Some("Dockerfile".to_string())
        );
        assert_eq!(bare_file_token("list the files here", cwd), None);
        let _ = std::fs::remove_dir_all(&dir);
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

    /// Selected text sent as context is never part of the command.
    #[test]
    fn command_from_instruction_drops_the_selected_text() {
        let ask = "run git status\n\nUse this text as context:\n\"\"\"\n$(touch /tmp/x)\n\"\"\"";
        assert_eq!(command_from_instruction(ask), "git status");
        let ask = "cargo test\n\n[session context]\nprevious goal: x";
        assert_eq!(command_from_instruction(ask), "cargo test");
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
            gate: Arc::new(Mutex::new(ConfirmationGate::default())),
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
        let (started, terminal) = drive_goal_event(bus, rx, payload).await;
        (started.payload, terminal.payload)
    }

    async fn drive_goal_event(
        bus: &EventBus,
        rx: &mut tokio::sync::broadcast::Receiver<Event>,
        payload: serde_json::Value,
    ) -> (Event, Event) {
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
                "goal.started" => started = Some(ev),
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev);
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

    #[tokio::test]
    async fn unconfirmed_terminal_goal_publishes_goal_failed_with_needs_confirmation() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        let (_started, terminal) =
            drive_goal_event(&bus, &mut rx, json!({ "goal": "git status" })).await;

        assert_eq!(terminal.kind, "goal.failed");
        assert_eq!(
            terminal.payload["needs_confirmation"],
            json!({
                "tool": "terminal",
                "action": "git status"
            })
        );
        assert_eq!(terminal.payload["error"], "Confirm to run: git status");
    }

    #[tokio::test]
    async fn confirmed_terminal_goal_with_matching_action_runs() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        let (_started, terminal) = drive_goal_event(
            &bus,
            &mut rx,
            json!({ "goal": "git status", "confirm": "git status" }),
        )
        .await;

        assert_eq!(terminal.kind, "goal.completed");
    }

    #[tokio::test]
    async fn mismatched_confirm_string_runs_nothing_and_requests_confirmation() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        let (_started, terminal) = drive_goal_event(
            &bus,
            &mut rx,
            json!({ "goal": "git status", "confirm": "git log" }),
        )
        .await;

        assert_eq!(terminal.kind, "goal.failed");
        assert_eq!(
            terminal.payload["needs_confirmation"],
            json!({
                "tool": "terminal",
                "action": "git status"
            })
        );
        assert_eq!(terminal.payload["error"], "Confirm to run: git status");
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
                "goal": "git status",
                "confirm": "git status",
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
                "goal": "git status",
                "confirm": "git status",
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
                "goal": "git status",
                "confirm": "git status",
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
            json!({ "goal": "git status", "confirm": "git status" }),
        )
        .await;
        assert_eq!(started["session_id"], DEFAULT_SESSION_ID);
        assert_eq!(started["session_turn"], 1);
        assert_eq!(done["session_id"], DEFAULT_SESSION_ID);
        assert_eq!(done["result"]["kind"], "text");

        let (started2, _) = drive_goal(
            &bus,
            &mut rx,
            json!({ "goal": "git status", "confirm": "git status" }),
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

    // ── S3 human control ─────────────────────────────────────────

    /// S3 acceptance: a `task.cancel` matching the in-flight goal id must stop
    /// the running goal — the Executive's cooperative checkpoint skips the
    /// remaining actions — and the runner publishes the real state transition
    /// `goal.cancelled`. No `goal.completed`/`goal.failed` ever follows (the
    /// portal card must not hang OR resolve as success/failure).
    #[tokio::test]
    async fn task_cancel_stops_the_in_flight_goal_and_publishes_goal_cancelled() {
        // A delayed mock: the web goal is still awaiting its (slow) web action
        // when the cancel lands, so the mid-execution checkpoint fires
        // deterministically.
        let (endpoint, server) = mock_search_endpoint(1500, 2);

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_web_endpoint(bus.clone(), endpoint);
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Search the web for the latest Rust async docs" }),
        ))
        .await
        .expect("publish");

        let mut goal_id = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.started" {
                goal_id = ev
                    .payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                break;
            }
        }
        let goal_id = goal_id.expect("goal.started must be published");

        // Cancel the in-flight goal by its real run id.
        bus.publish(Event::new(
            "task.cancel",
            "mobile",
            json!({ "task_id": goal_id }),
        ))
        .await
        .expect("publish");

        let mut saw_cancelled = false;
        let mut saw_terminal = false;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.cancelled" => {
                    saw_cancelled = true;
                    assert_eq!(ev.payload["id"], goal_id);
                    assert_eq!(ev.payload["reason"], "cancelled_by_human");
                }
                "goal.completed" | "goal.failed" if ev.payload["id"] == goal_id => {
                    saw_terminal = true;
                }
                _ => {}
            }
            if saw_cancelled && saw_terminal {
                break;
            }
        }
        let _ = server.join();

        assert!(saw_cancelled, "task.cancel must produce goal.cancelled");
        assert!(
            !saw_terminal,
            "a cancelled goal must never publish completed/failed"
        );
    }

    /// S3: `task.pause` matching the in-flight goal id stops the run and the
    /// runner publishes `goal.paused` — the same cooperative stop as cancel,
    /// with the distinct terminal kind the portal renders as Paused.
    #[tokio::test]
    async fn task_pause_stops_the_in_flight_goal_and_publishes_goal_paused() {
        let (endpoint, server) = mock_search_endpoint(1500, 2);

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_web_endpoint(bus.clone(), endpoint);
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Search the web for the latest Rust async docs" }),
        ))
        .await
        .expect("publish");

        let mut goal_id = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.started" {
                goal_id = ev
                    .payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                break;
            }
        }
        let goal_id = goal_id.expect("goal.started must be published");

        bus.publish(Event::new(
            "task.pause",
            "mobile",
            json!({ "task_id": goal_id }),
        ))
        .await
        .expect("publish");

        let mut saw_paused = false;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.paused" {
                saw_paused = true;
                assert_eq!(ev.payload["id"], goal_id);
                assert_eq!(ev.payload["reason"], "paused_by_human");
                break;
            }
        }
        let _ = server.join();

        assert!(saw_paused, "task.pause must produce goal.paused");
    }

    /// S3: a sensitive goal completes WITHOUT storing (the episode is held for
    /// a human), the `goal.completed` payload carries the held episode's id,
    /// and a `task.approve` for that id admits the episode into memory and
    /// publishes `goal.approved` — the approve control request produces a real
    /// state transition, not a silent ack.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn task_approve_admits_a_held_episode_and_publishes_goal_approved() {
        let (endpoint, server) = mock_search_endpoint(0, 2);

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_web_endpoint(bus.clone(), endpoint);
        runner.start();

        // The goal text carries a sensitivity marker → the admission gate
        // holds the episode for a human instead of silently storing it.
        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "Search the web for how to reset my password safely" }),
        ))
        .await
        .expect("publish");

        let mut saw_terminal = false;
        let mut pending_approval_id = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.completed" {
                saw_terminal = true;
                assert_eq!(
                    ev.payload["pending_approval"],
                    json!(true),
                    "the completed payload surfaces the held episode"
                );
                pending_approval_id = ev
                    .payload
                    .get("pending_approval_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                break;
            }
        }
        let _ = tokio::task::spawn_blocking(move || server.join()).await;

        assert!(saw_terminal, "the sensitive goal must complete");
        let pending_id = pending_approval_id.expect("pending_approval_id in the payload");

        // Approve the held episode by its id → admitted + goal.approved.
        bus.publish(Event::new(
            "task.approve",
            "mobile",
            json!({ "task_id": pending_id }),
        ))
        .await
        .expect("publish");

        let mut saw_approved = false;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(SLOW_RUNNER_WAIT, rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.approved" {
                saw_approved = true;
                assert_eq!(ev.payload["task_id"], pending_id);
                break;
            }
        }
        assert!(saw_approved, "task.approve must publish goal.approved");
    }

    /// S3 honesty: a `task.approve` whose id matches no held episode must be a
    /// no-op — never a fabricated `goal.approved`. The state transition only
    /// exists when there is a real held episode to approve.
    #[tokio::test]
    async fn task_approve_for_unknown_id_is_ignored() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::new(bus.clone());
        runner.start();

        bus.publish(Event::new(
            "task.approve",
            "mobile",
            json!({ "task_id": "ep-unknown" }),
        ))
        .await
        .expect("publish");

        // The control loop processes it within milliseconds; an idle window
        // proves nothing was fabricated.
        let mut saw_approved = false;
        while let Ok(ev) = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
            .await
            .map(|r| r.expect("bus alive"))
        {
            if ev.kind == "goal.approved" {
                saw_approved = true;
            }
        }
        assert!(
            !saw_approved,
            "approving an unknown id must not fabricate a transition"
        );
    }

    /// S4: "open notepad" travels the whole live-kernel path — plan → the
    /// `windows.open` capability → Executive → WindowsAdapter → mock host →
    /// verified → `goal.completed`. The MockTransport stands in for the C# host
    /// (accepts `windows.open` with the resolved `path`, returns a window), so
    /// the acceptance is hermetic and never touches a real pipe.
    #[tokio::test]
    async fn open_notepad_goal_completes_through_windows_host() {
        use friday_windows::ipc::MockTransport;
        use friday_windows::protocol::BridgeResponse;

        let launched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let launched_clone = launched.clone();
        let adapter = WindowsAdapter::new(Arc::new(MockTransport::new(Arc::new(
            move |frame: friday_windows::protocol::BridgeFrame| {
                let intent = frame.message.action.clone();
                let path = frame
                    .message
                    .payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                launched_clone
                    .lock()
                    .unwrap()
                    .push(format!("{intent}:{path}"));
                BridgeResponse::ok(json!({
                    "hwnd": 42,
                    "title": "Untitled - Notepad",
                    "pid": 1234,
                }))
            },
        ))));

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            Some(adapter),
            None,
            None,
        );
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "open notepad", "confirm": "notepad.exe" }),
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
        assert_eq!(
            ev.kind, "goal.completed",
            "windows goal must complete, not fail: {ev:?}"
        );
        let result = ev.payload["result"].as_object().expect("result object");
        assert_eq!(result["kind"], "text");
        assert_eq!(result["tool"], "windows", "served by the Windows host tool");
        let calls = launched.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["windows.open:notepad.exe"],
            "the adapter must have sent the resolved launch to the host"
        );
    }

    /// S5-A (runner level): the host fails the first launch (the partial side
    /// effect is already recorded) then succeeds on the retry — the recovery
    /// loop re-observes, retries, verifies, and publishes `goal.completed` with
    /// `recovered: true` and both attempts visible in the adapter call log.
    #[tokio::test]
    async fn windows_goal_recovers_after_a_transient_host_failure() {
        use friday_windows::ipc::MockTransport;
        use friday_windows::protocol::BridgeResponse;

        let launched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let calls = Arc::new(std::sync::Mutex::new(0u32));
        let launched_clone = launched.clone();
        let calls_clone = calls.clone();
        let adapter = WindowsAdapter::new(Arc::new(MockTransport::new(Arc::new(
            move |frame: friday_windows::protocol::BridgeFrame| {
                let intent = frame.message.action.clone();
                let path = frame
                    .message
                    .payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                launched_clone
                    .lock()
                    .unwrap()
                    .push(format!("{intent}:{path}"));
                let mut n = calls_clone.lock().unwrap();
                *n += 1;
                if *n == 1 {
                    BridgeResponse::err("app_not_found")
                } else {
                    BridgeResponse::ok(json!({
                        "hwnd": 42,
                        "title": "Untitled - Notepad",
                        "pid": 1234,
                    }))
                }
            },
        ))));

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            Some(adapter),
            None,
            None,
        );
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "open notepad", "confirm": "notepad.exe" }),
        ))
        .await
        .expect("publish");

        let mut terminal = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev);
                    break;
                }
                _ => {}
            }
        }

        let ev = terminal.expect("a terminal event");
        assert_eq!(
            ev.kind, "goal.completed",
            "a recovered goal completes, not fails: {ev:?}"
        );
        assert_eq!(
            ev.payload["recovered"],
            json!(true),
            "recovery evidence on the payload: {ev:?}"
        );
        assert_eq!(
            ev.payload["recovery_attempts"],
            json!(2),
            "fail once, succeed once: {ev:?}"
        );
        let calls = launched.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["windows.open:notepad.exe".to_string(); 2],
            "both attempts reached the host — the partial side effect then the retry"
        );
    }

    /// S5-B (runner level): the host fails every launch — the recovery loop
    /// hits its hard ceiling and the runner publishes `goal.failed` carrying
    /// the recovery evidence: `recoverable: true`, the failed reason, the
    /// attempt count, and the last observed world state.
    #[tokio::test]
    async fn windows_goal_exhausts_recovery_and_fails_recoverable() {
        use friday_windows::ipc::MockTransport;
        use friday_windows::protocol::BridgeResponse;

        let launched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let launched_clone = launched.clone();
        let adapter = WindowsAdapter::new(Arc::new(MockTransport::new(Arc::new(
            move |frame: friday_windows::protocol::BridgeFrame| {
                launched_clone.lock().unwrap().push(format!(
                    "{}:{}",
                    frame.message.action,
                    frame
                        .message
                        .payload
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ));
                BridgeResponse::err("app_not_found")
            },
        ))));

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            Some(adapter),
            None,
            None,
        );
        runner.start();

        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({ "goal": "open notepad", "confirm": "notepad.exe" }),
        ))
        .await
        .expect("publish");

        let mut terminal = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            match ev.kind.as_str() {
                "goal.completed" | "goal.failed" => {
                    terminal = Some(ev);
                    break;
                }
                _ => {}
            }
        }

        let ev = terminal.expect("a terminal event");
        assert_eq!(
            ev.kind, "goal.failed",
            "an exhausted recovery is a real failure, never a phantom completion: {ev:?}"
        );
        assert_eq!(ev.payload["recoverable"], json!(true), "{ev:?}");
        assert_eq!(
            ev.payload["recovery_attempts"],
            json!(3),
            "one initial attempt plus the bounded retries: {ev:?}"
        );
        assert!(
            ev.payload["failed_reason"].is_string(),
            "failed_reason explains the exhaustion: {ev:?}"
        );
        assert!(
            ev.payload["last_observed_state"].is_object(),
            "the last observed state is carried for a follow-up goal: {ev:?}"
        );
        assert_eq!(
            launched.lock().unwrap().len(),
            3,
            "every bounded attempt reached the host — no infinite retry machine"
        );
    }

    /// S6-B (runner level): the laptop path perceives the machine. A real
    /// Observe phase runs inside the actual GoalRunner with a sensor loop
    /// attached — the screen observation must reach the blackboard WorldState,
    /// surface as an attention item, be reported as `observations_folded`, and
    /// the fed goal must still win focus (an observation is never allowed to
    /// hijack a user goal).
    #[tokio::test]
    async fn runner_observes_sensors_into_world_state_without_hijacking_the_goal() {
        use friday_cognitive::workspace::Topic;
        use friday_cognitive::WorkspaceEvent;
        use friday_sensors::MockSensor;
        use friday_windows::ipc::MockTransport;
        use friday_windows::protocol::BridgeResponse;

        let launched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let launched_clone = launched.clone();
        let adapter = WindowsAdapter::new(Arc::new(MockTransport::new(Arc::new(
            move |frame: friday_windows::protocol::BridgeFrame| {
                launched_clone.lock().unwrap().push(format!(
                    "{}:{}",
                    frame.message.action,
                    frame
                        .message
                        .payload
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ));
                BridgeResponse::ok(json!({
                    "hwnd": 42,
                    "title": "Untitled - Notepad",
                    "pid": 1234,
                }))
            },
        ))));

        // The laptop's observation seam: the same SensorLoop type the cognitive
        // runtime uses, with a deterministic screen sensor standing in for the
        // real WindowsScreenSensor.
        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(MockSensor::new(
            "mock_screen",
            vec![WorkspaceEvent::new(
                Topic::Screen,
                json!({
                    "title": "Terminal",
                    "process": "terminal",
                    "change": "content",
                }),
            )],
        )));

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            Some(adapter),
            None,
            Some(Observer::new(loop_)),
        );

        runner
            .run_goal(
                "open notepad".to_string(),
                DEFAULT_SESSION_ID.to_string(),
                Some("notepad.exe".to_string()),
                AskOptions::default(),
            )
            .await;

        // The Observe phase folded the sensor event into the blackboard WorldState.
        let world = runner.workspace.get_world_state().expect("world state");
        assert_eq!(
            world.variables["screen"]["process"], "terminal",
            "the real Observe phase must land the observation in the WorldState"
        );
        assert!(
            world.has_tool(&Capability::Custom("windows.open".into())),
            "the observation fold must never drop the advertised world surface"
        );

        // The observation surfaced as an attention item…
        let attention = runner.workspace.get_attention();
        assert!(
            attention
                .iter()
                .any(|i| i.description.contains("screen: terminal")),
            "the observed change must surface as an attention item: {:?}",
            attention
        );
        // …while the fed goal stayed in the ranking too (it was never evicted
        // by a sensor observation).
        assert!(
            attention
                .iter()
                .any(|i| i.kind == friday_goal_execution::AttentionItemKind::Goal),
            "the fed goal must remain an attention item: {:?}",
            attention
        );

        // The goal — not the observation — drove execution: the host was asked
        // to launch notepad exactly once. A hijacked focus would have run the
        // screen description instead and left the host untouched.
        assert_eq!(
            launched.lock().unwrap().clone(),
            vec!["windows.open:notepad.exe".to_string()],
            "the user goal must win focus over the sensor observation"
        );

        // The completed goal reports the folded-observation count honestly.
        let mut completed = None;
        while let Ok(ev) = rx.recv().await {
            if ev.kind == "goal.completed" {
                completed = Some(ev);
                break;
            }
        }
        let ev = completed.expect("a completed goal event");
        assert_eq!(ev.payload["observations_folded"], json!(1), "{ev:?}");
    }

    /// S8-A acceptance: the voice intake drives the *existing* goal loop end to
    /// end. Synthetic PCM → `MockTranscriber` → `TurnDetected("open Chrome")`
    /// → `VoiceIntake` → `plan.goal` (the same intake the portal uses) →
    /// `GoalRunner` → planning → execution → verification → `goal.completed`.
    ///
    /// The transcript arrives verbatim and the loop answers exactly like a
    /// portal goal — the voice goes through the same capability machinery, not
    /// a voice→tool shortcut. The host launch goes over the same Windows bridge
    /// mock the existing "open notepad" test uses.
    #[tokio::test]
    async fn voice_turn_drives_the_existing_goal_loop_to_goal_completed() {
        use crate::voice::test_turn_pcm;
        use crate::VoiceIntake;
        use friday_sensors::audio::{AudioSensor, MockTranscriber, SpeechToText};
        use friday_windows::ipc::MockTransport;
        use friday_windows::protocol::BridgeResponse;

        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        let launched = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let launched_clone = launched.clone();
        let adapter = WindowsAdapter::new(Arc::new(MockTransport::new(Arc::new(
            move |frame: friday_windows::protocol::BridgeFrame| {
                launched_clone
                    .lock()
                    .unwrap()
                    .push(frame.message.action.clone());
                BridgeResponse::ok(json!({
                    "hwnd": 99,
                    "title": "Chrome",
                    "pid": 4242,
                }))
            },
        ))));

        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            None,
            None,
            Some(adapter),
            None,
            None,
        );
        runner.start();

        // The ear: the existing AudioSensor behind the frozen SpeechToText seam,
        // driven hermetically over synthetic PCM.
        let asr: Arc<dyn SpeechToText> = Arc::new(MockTranscriber {
            phrase: "open Chrome".into(),
        });
        let mut voice = VoiceIntake::new(bus.clone(), AudioSensor::without_capture().with_asr(asr));
        voice.feed_pcm(&test_turn_pcm());

        // The turn becomes a plan.goal on the intake — raw words, no plan made.
        let admitted = voice.poll().await;
        assert_eq!(admitted, vec!["open Chrome".to_string()]);

        // The goal loop gates the unconfirmed action and requests confirmation.
        let mut needs_confirm = false;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.failed" && ev.payload.get("needs_confirmation").is_some() {
                needs_confirm = true;
                break;
            }
        }
        assert!(needs_confirm, "voice goal must require confirmation");

        // The human confirms the action -> completes.
        bus.publish(Event::new(
            "plan.goal",
            "mobile",
            json!({
                "goal": "open Chrome",
                "confirm": "chrome.exe",
                "session_id": VoiceIntake::DEFAULT_SESSION_ID,
            }),
        ))
        .await
        .expect("publish");

        let mut completed = None;
        for _ in 0..30 {
            let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .map(|r| r.expect("bus alive"))
            else {
                break;
            };
            if ev.kind == "goal.completed" {
                completed = Some(ev);
                break;
            }
        }
        let ev = completed.expect("voice goal must complete");
        assert_eq!(ev.payload["session_id"], VoiceIntake::DEFAULT_SESSION_ID);
        assert!(ev.payload["result"].is_object(), "a real result: {ev:?}");
        assert_eq!(
            launched.lock().unwrap().clone(),
            vec!["windows.open".to_string()],
            "the spoken goal ran through the existing host-launch capability, once"
        );
    }

    /// S8-C hermetic: the *live* voice wiring — an enabled [`VoiceConfig`]
    /// attaches the real [`WhisperCppTranscriber`] behind the observer's
    /// [`AudioSensor`] — proven **without requiring a physical microphone**.
    ///
    /// The no-capture sensor drives a full observer fold (the exact shape the
    /// daemon's `live_observer` adds to the loop). A host with no mic therefore
    /// behaves as honest silence: no capture → no events → the WorldState fold
    /// is the identity. Voice is configuration, never a requirement — exactly
    /// the S8-C failure semantics.
    #[tokio::test]
    async fn enabled_voice_wires_whisper_into_the_observer_without_a_mic() {
        use friday_goal_execution::fold_observation;
        use friday_sensors::whisper_cpp::WhisperCppConfig;

        let voice = VoiceConfig::Enabled(WhisperCppConfig::new("whisper-cli", "ggml-base.bin"));
        assert!(voice.enabled());

        // The same sensor `VoiceConfig::sensor()` hands the live observer,
        // minus the physical capture device — hermetic proof of the wiring.
        let mic = voice
            .sensor_without_capture()
            .expect("enabled voice must build a sensor");
        assert!(mic.asr_attached(), "Whisper must be the ASR backend");
        assert!(!mic.capture_available(), "no physical mic needed");

        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(mic));
        let mut observer = Observer::new(loop_);

        // No mic → no events → the fold invents nothing (VAD-only honesty).
        let events = observer.poll().await;
        assert!(events.is_empty(), "a no-capture loop reports nothing");
        let base = WorldState::new();
        let (state, descriptions) = fold_observation(base.clone(), &events);
        assert_eq!(state, base, "the fold is the identity without a mic");
        assert!(descriptions.is_empty(), "no invented attention items");
    }

    /// S8-C: a disabled or misconfigured voice config attaches **nothing** to
    /// the live loop — the exact pre-S8-C behavior, so voice can never take
    /// the daemon down.
    #[test]
    fn disabled_or_misconfigured_voice_attaches_no_sensor() {
        for voice in [
            VoiceConfig::Disabled,
            VoiceConfig::Misconfigured("FRIDAY_WHISPER_EXE is missing".into()),
        ] {
            assert!(voice.sensor().is_none(), "{voice:?} must stay offline");
            assert!(
                voice.sensor_without_capture().is_none(),
                "{voice:?} must stay offline"
            );
            assert!(!voice.enabled(), "{voice:?} must not report enabled");
        }
    }

    /// S7-B: the real laptop runner's learning loop. The same web-research
    /// goal runs four times against a hermetic mock search endpoint:
    ///
    ///   runs 1-3 → each executes an evidence-gated plan and completes
    ///   verified → SemanticMemory accumulates the `web.search → docs.search`
    ///   workflow → it promotes at support 3 (`policies_derived` fills).
    ///
    ///   run 4 → the goal classifies to `SearchWeb`, which the relevance gate
    ///   recognizes as the `web.*` channel skill the learned policy starts
    ///   with → `policies_applied` fills → the `PolicyDriven` plan
    ///   `web.search → docs.search` wins the ranking (same 0.75 confidence,
    ///   one action shorter than the 3-step evidence plan), executes against
    ///   the same hermetic tools, verifies, and completes.
    ///
    /// The stored episode carries the REAL GoalEngine insight — the goal's own
    /// context in the statement — never the old fabricated "builtin"
    /// reflection. The mock body is deliberately keyword-free (`Tokio
    /// runtime`, `Rust home`) so a recalled memory line can never inflate the
    /// investigation (a title like "documentation" would cap the docs
    /// requirement at 1.0 and the evidence plan would beat the learned policy).
    #[tokio::test]
    async fn runner_learns_and_reuses_a_web_workflow_policy() {
        use std::io::{Read, Write};

        let body = r#"<html><body>
            <a href="https://tokio.rs/">Tokio runtime</a>
            <a href="https://rust-lang.org/">Rust home</a>
        </body></html>"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking mock server");
        let addr = listener.local_addr().expect("mock server address");
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let mut served = 0usize;
            let mut last_activity = std::time::Instant::now();
            while std::time::Instant::now() < deadline && served < 20 {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if last_activity.elapsed() > std::time::Duration::from_secs(2) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
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
        let runner = GoalRunner::with_investigation_engine(
            bus.clone(),
            InvestigationEngine::heuristic(),
            None,
            Some(format!("http://{addr}/")),
            None,
            None,
            None,
            None,
        );

        let goal = "Search the web for the Rust async docs".to_string();
        let mut derived: Vec<u64> = Vec::new();
        let mut applied: Vec<u64> = Vec::new();

        for run in 1..=4u32 {
            runner
                .run_goal(
                    goal.clone(),
                    format!("s7-session-{run}"),
                    None,
                    AskOptions::default(),
                )
                .await;
            let mut terminal = None;
            for _ in 0..30 {
                let Ok(ev) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                    .await
                    .map(|r| r.expect("bus alive"))
                else {
                    break;
                };
                if ev.kind == "goal.completed" || ev.kind == "goal.failed" {
                    terminal = Some(ev);
                    break;
                }
            }
            let ev = terminal.unwrap_or_else(|| panic!("run {run} must reach a terminal event"));
            assert_eq!(
                ev.kind, "goal.completed",
                "run {run} must complete, not fail: {ev:?}"
            );
            assert_eq!(
                ev.payload["result"]["kind"], "text",
                "run {run} must complete with a result: {ev:?}"
            );
            derived.push(ev.payload["policies_derived"].as_u64().unwrap_or(0));
            applied.push(ev.payload["policies_applied"].as_u64().unwrap_or(0));
        }
        let _ = server.join();

        // Runs 1-2: planning precedes the cycle's Learn, and support 3 has not
        // been reached — nothing consolidated, nothing qualified.
        assert_eq!(derived[0], 0, "run 1 consolidated nothing yet: {derived:?}");
        assert_eq!(
            derived[1], 0,
            "run 2 is still below the support threshold: {derived:?}"
        );
        assert!(
            derived[2] >= 1,
            "run 3 must promote the workflow: {derived:?}"
        );
        for (i, a) in applied.iter().enumerate().take(3) {
            assert_eq!(
                *a,
                0,
                "run {} had no eligible policy yet: {applied:?}",
                i + 1
            );
        }
        // Run 4: SearchWeb ⇄ Custom("web.search") — the learned policy
        // qualified for planning and won.
        assert!(
            applied[3] >= 1,
            "run 4 must apply the learned policy: {applied:?}"
        );

        // Run 4's chosen plan is the learned transition — one action shorter
        // than the evidence plan at the same confidence.
        let plan = runner.workspace.get_active_plan().expect("run 4 plan");
        assert_eq!(
            plan.source,
            friday_goal_execution::PlanSource::PolicyDriven,
            "{plan:?}"
        );
        assert_eq!(plan.actions.len(), 2, "{plan:?}");
        assert_eq!(
            plan.actions[0].capability,
            Capability::Custom("web.search".into()),
            "{plan:?}"
        );
        assert_eq!(
            plan.actions[1].capability,
            Capability::Custom("docs.search".into()),
            "{plan:?}"
        );

        // The stored episode's reflection is the REAL GoalEngine insight — it
        // names the goal that ran; the fabricated "builtin" is gone.
        let reflection = {
            let exec = runner.executive.lock().await;
            let episodes = exec.episode_store().episodes();
            let latest = episodes.last().expect("run 4 stored an episode");
            latest
                .reflection
                .clone()
                .expect("run 4 episode records a real reflection")
        };
        assert!(!reflection.is_empty(), "reflection must not be empty");
        assert!(
            !reflection.contains("builtin"),
            "the fabricated builtin reflection must be gone: {reflection:?}"
        );
        assert!(
            reflection.contains("Search the web for the Rust async docs"),
            "the reflection must derive from the real goal context: {reflection:?}"
        );
    }

    /// S4: the browser tool advertises ONLY the safe `web.open` verb (the
    /// adapter's high-risk `web.evaluate` stays off the surface) and refuses an
    /// instruction with no URL instead of guessing or launching Chrome.
    #[tokio::test]
    async fn browser_tool_advertises_safe_surface_and_needs_a_url() {
        let tool = BrowserCapabilityTool {
            adapter: BrowserAdapter::new(),
        };
        assert_eq!(
            tool.capabilities(),
            vec![Capability::Custom("web.open".into())],
            "only the safe web.open verb may be advertised"
        );

        let action = Action::single(
            "open the browser",
            Capability::Custom("web.open".into()),
            "goal-1",
        );
        let res = tool
            .execute(&action, &ExecutionContext::for_goal("goal-1"))
            .await;
        assert!(!res.success, "must refuse, not launch a browser: {res:?}");
        assert!(
            res.error.clone().unwrap_or_default().contains("no URL"),
            "refusal must name the missing URL: {res:?}"
        );
    }

    /// S4: the instruction parsers resolve friendly names / addresses the way
    /// the host contract expects.
    #[test]
    fn app_from_instruction_resolves_names_to_executables() {
        assert_eq!(app_from_instruction("open notepad"), "notepad.exe");
        assert_eq!(app_from_instruction("launch chrome"), "chrome.exe");
        assert_eq!(app_from_instruction("Open the calculator app"), "calc.exe");
        assert_eq!(app_from_instruction("open notepad.exe"), "notepad.exe");
        assert_eq!(app_from_instruction("open edge"), "msedge.exe");
    }

    /// An app that is not in the table: its name goes to the host whole (or
    /// as `<name>.exe` for one word), without the words around it.
    #[test]
    fn app_from_instruction_keeps_unknown_names_and_drops_filler() {
        assert_eq!(
            app_from_instruction("open brave browser on this device"),
            "brave.exe"
        );
        assert_eq!(app_from_instruction("open brave"), "brave.exe");
        assert_eq!(
            app_from_instruction("launch microsoft teams"),
            "microsoft teams"
        );
        assert_eq!(app_from_instruction("open Visual Studio Code"), "code.exe");
        assert_eq!(
            app_from_instruction("please open spotify for me"),
            "spotify.exe"
        );
        assert_eq!(
            app_from_instruction("open the google chrome app"),
            "chrome.exe"
        );
    }

    #[test]
    fn a_failed_open_says_what_happened_in_words() {
        let msg = open_failure("brave.exe", "AppNotFound");
        assert!(
            msg.contains("couldn't find an app called \"brave\""),
            "{msg}"
        );
        assert!(open_failure("x.exe", "HostUnavailable").contains("isn't running"));
        assert_eq!(open_failure("x.exe", "Unknown:boom"), "boom");
    }

    #[test]
    fn url_from_instruction_extracts_addresses() {
        assert_eq!(
            url_from_instruction("open example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            url_from_instruction("open https://example.org").as_deref(),
            Some("https://example.org")
        );
        assert_eq!(
            url_from_instruction("browse to www.rust-lang.org").as_deref(),
            Some("https://www.rust-lang.org")
        );
        assert_eq!(url_from_instruction("open notepad"), None);
    }

    #[test]
    fn goal_runner_is_idle_reports_status() {
        let bus = EventBus::new();
        let runner = GoalRunner::new(bus.clone());
        assert!(runner.is_idle());

        let handle = runner.handle();
        assert!(handle.is_idle());

        *handle.in_flight.lock().unwrap() = Some(InFlightGoal {
            id: "goal-test-1".to_string(),
        });
        assert!(!runner.is_idle());
        assert!(!handle.is_idle());

        *handle.in_flight.lock().unwrap() = None;
        assert!(runner.is_idle());
        assert!(handle.is_idle());
    }

    #[tokio::test]
    async fn goal_in_flight_is_cleared_after_terminal_event() {
        let bus = EventBus::new();
        let mut sub = bus.subscribe();
        let runner = Arc::new(GoalRunner::new(bus.clone()));
        let handle = runner.handle();

        let runner_clone = runner.clone();
        let task = tokio::spawn(async move {
            runner_clone
                .run_goal(
                    "run echo ok".to_string(),
                    "session-f".to_string(),
                    None,
                    AskOptions::default(),
                )
                .await;
        });

        // The terminal event must arrive from the bus.
        let mut received_terminal = false;
        while let Ok(event) = sub.recv().await {
            if event.kind == "goal.completed" || event.kind == "goal.failed" {
                received_terminal = true;
                break;
            }
        }
        assert!(received_terminal, "must receive terminal event");
        let _ = task.await;
        assert!(handle.is_idle());
    }
}
