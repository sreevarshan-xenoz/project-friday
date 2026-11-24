//! MCP client servers: connect to external MCP servers configured in
//! `mcp-servers.json`, filtered by `mcp-on`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// An external MCP server configured in `mcp-servers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: BTreeMap<String, RawServerEntry>,
}

#[derive(Debug, Deserialize)]
struct RawServerEntry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Status of a configured MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub name: String,
    pub on: bool,
}

/// A tool advertised by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub server: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// A tool picked by the model to answer a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub server: String,
    pub tool: String,
    pub arguments: serde_json::Value,
}

/// FRIDAY's config folder (`$XDG_CONFIG_HOME/friday`, or `%APPDATA%\friday`).
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_default().join("friday")
}

/// `$XDG_CONFIG_HOME/friday/mcp-servers.json`
pub fn servers_file() -> PathBuf {
    config_dir().join("mcp-servers.json")
}

/// `$XDG_CONFIG_HOME/friday/mcp-on`
pub fn on_file() -> PathBuf {
    config_dir().join("mcp-on")
}

/// Parse Claude Desktop config format: `{"mcpServers": {"<name>": {"command": "...", "args": [...], "env": {...}}}}`.
/// Returns `(map, None)` on success, or `(empty_map, Some(error))` on bad JSON.
pub fn parse_config(raw: &str) -> (BTreeMap<String, ServerConfig>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (BTreeMap::new(), None);
    }
    match serde_json::from_str::<ClaudeDesktopConfig>(trimmed) {
        Ok(parsed) => {
            let mut servers = BTreeMap::new();
            for (name, entry) in parsed.mcp_servers {
                servers.insert(
                    name.clone(),
                    ServerConfig {
                        name,
                        command: entry.command,
                        args: entry.args,
                        env: entry.env,
                    },
                );
            }
            (servers, None)
        }
        Err(e) => (BTreeMap::new(), Some(e.to_string())),
    }
}

/// Parse the `mcp-on` file: one server name per line.
pub fn parse_on(raw: &str) -> HashSet<String> {
    raw.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// List servers from config with their on/off status.
pub fn list_servers(config_raw: &str, on_raw: &str) -> Vec<ServerStatus> {
    let (servers, _) = parse_config(config_raw);
    let on_set = parse_on(on_raw);
    servers
        .into_keys()
        .map(|name| {
            let on = on_set.contains(&name);
            ServerStatus { name, on }
        })
        .collect()
}

/// Read and parse `mcp-servers.json`.
pub fn load_servers() -> BTreeMap<String, ServerConfig> {
    let raw = std::fs::read_to_string(servers_file()).unwrap_or_default();
    let (servers, err) = parse_config(&raw);
    if let Some(e) = err {
        tracing::warn!("failed to parse {}: {}", servers_file().display(), e);
    }
    servers
}

/// Read servers that are switched on in `mcp-on`.
pub fn load_active_servers() -> Vec<ServerConfig> {
    let servers = load_servers();
    let on_raw = std::fs::read_to_string(on_file()).unwrap_or_default();
    let on_set = parse_on(&on_raw);
    servers
        .into_values()
        .filter(|s| on_set.contains(&s.name))
        .collect()
}

/// Whether at least one configured server is switched on.
pub fn has_servers_on() -> bool {
    let on_raw = std::fs::read_to_string(on_file()).unwrap_or_default();
    let on_set = parse_on(&on_raw);
    if on_set.is_empty() {
        return false;
    }
    let servers = load_servers();
    servers.keys().any(|k| on_set.contains(k))
}

/// Connect to an MCP server, initialize, and list its tools with a 10s timeout.
pub async fn connect(server: &ServerConfig) -> Result<Vec<ToolInfo>, String> {
    let fut = async {
        let (transport, _child) = friday_mcp::transport::stdio_transport_for_with_env(
            &server.command,
            &server.args,
            &server.env,
        )
        .map_err(|e| format!("failed to launch MCP server '{}': {e}", server.name))?;

        let mut client = friday_mcp::protocol::McpClient::new(transport);
        client
            .initialize()
            .await
            .map_err(|e| format!("MCP initialize failed for '{}': {e}", server.name))?;
        let tools = client
            .list_tools()
            .await
            .map_err(|e| format!("MCP list_tools failed for '{}': {e}", server.name))?;

        Ok(tools
            .into_iter()
            .map(|t| ToolInfo {
                server: server.name.clone(),
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect())
    };

    match tokio::time::timeout(std::time::Duration::from_secs(10), fut).await {
        Ok(res) => res,
        Err(_) => Err(format!("MCP server '{}' timed out after 10s", server.name)),
    }
}

/// The tools of the servers switched on, with the names of those servers:
/// `friday mcp on|off` changes the names, so the next ask lists them again.
type ToolsCache = Option<(Vec<String>, Vec<ToolInfo>)>;
static TOOLS_CACHE: tokio::sync::Mutex<ToolsCache> = tokio::sync::Mutex::const_new(None);

/// Get tools from all switched-on servers, connecting lazily; kept until the
/// set of servers switched on changes.
pub async fn get_tools() -> Vec<ToolInfo> {
    let active = load_active_servers();
    let names: Vec<String> = active.iter().map(|s| s.name.clone()).collect();
    if let Some((cached_names, cached)) = &*TOOLS_CACHE.lock().await {
        if *cached_names == names {
            return cached.clone();
        }
    }
    // Connected without holding the cache, so a slow server holds up nothing else.
    let mut all_tools = Vec::new();
    for server in active {
        match connect(&server).await {
            Ok(tools) => all_tools.extend(tools),
            Err(e) => {
                tracing::warn!(server = %server.name, error = %e, "MCP server skipped");
            }
        }
    }
    *TOOLS_CACHE.lock().await = Some((names, all_tools.clone()));
    all_tools
}

#[cfg(test)]
pub async fn clear_tools_cache() {
    let mut lock = TOOLS_CACHE.lock().await;
    *lock = None;
}

/// Ask the model to pick a tool for the question, or reply NONE.
pub fn pick_prompt(question: &str, tools: &[ToolInfo]) -> String {
    let mut catalog = String::new();
    for t in tools {
        let schema = serde_json::to_string(&t.input_schema).unwrap_or_else(|_| "{}".to_string());
        catalog.push_str(&format!(
            "- Server: {}\n  Tool: {}\n  Description: {}\n  inputSchema: {}\n\n",
            t.server, t.name, t.description, schema
        ));
    }
    format!(
        r#"Given the user question below and the available MCP tools, decide if one fits the request.

Available tools:
{catalog}
User question: {question}

Reply with ONLY a single JSON object in this exact format if a tool fits:
{{"server": "<server name>", "tool": "<tool name>", "arguments": {{...}}}}

If no tool fits, reply with ONLY:
NONE"#
    )
}

/// Parse the model's reply, accepting only a listed server+tool and an object for arguments.
pub fn parse_pick(reply: &str, tools: &[ToolInfo]) -> Option<Pick> {
    let trimmed = reply.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if trimmed.to_ascii_uppercase().starts_with("NONE")
        && trimmed.len() <= 6
        && trimmed[4..]
            .chars()
            .all(|c| c.is_ascii_punctuation() || c.is_whitespace())
    {
        return None;
    }

    let text = if let Some(start) = trimmed.find("```") {
        let after_start = &trimmed[start + 3..];
        let content = if let Some(nl) = after_start.find('\n') {
            &after_start[nl + 1..]
        } else {
            after_start
        };
        if let Some(end) = content.find("```") {
            &content[..end]
        } else {
            content
        }
    } else {
        trimmed
    };

    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if start > end {
        return None;
    }
    let json_slice = &text[start..=end];

    let val: serde_json::Value = serde_json::from_str(json_slice).ok()?;
    let obj = val.as_object()?;
    let server = obj.get("server")?.as_str()?;
    let tool = obj.get("tool")?.as_str()?;
    let arguments = obj.get("arguments")?;
    if !arguments.is_object() {
        return None;
    }

    if !tools.iter().any(|t| t.server == server && t.name == tool) {
        return None;
    }

    Some(Pick {
        server: server.to_string(),
        tool: tool.to_string(),
        arguments: arguments.clone(),
    })
}

/// The call a person confirmed ("<server>.<tool> <arguments json>", as the
/// confirmation gate showed it), so a confirmed run calls exactly that
/// instead of asking the model again (which could pick differently).
pub fn parse_confirmed(confirm: &str, tools: &[ToolInfo]) -> Option<Pick> {
    // Names can hold dots ("db" + "table.get" reads like "db.table" + "get"):
    // a confirmation that fits more than one tool runs none.
    let mut found = tools.iter().filter_map(|t| {
        let rest = confirm
            .trim()
            .strip_prefix(&format!("{}.{} ", t.server, t.name))?;
        let arguments: serde_json::Value = serde_json::from_str(rest).ok()?;
        arguments.is_object().then(|| Pick {
            server: t.server.clone(),
            tool: t.name.clone(),
            arguments,
        })
    });
    let pick = found.next()?;
    found.next().is_none().then_some(pick)
}

/// Call the picked tool, returning its text result capped at 8000 characters.
/// Arguments and results are never logged (lengths only).
pub async fn call(pick: &Pick) -> Result<String, String> {
    let servers = load_servers();
    let server = servers
        .get(&pick.server)
        .ok_or_else(|| format!("unknown MCP server '{}'", pick.server))?;
    call_on(server, pick).await
}

/// [`call`] on a server already looked up.
async fn call_on(server: &ServerConfig, pick: &Pick) -> Result<String, String> {
    let args_str = serde_json::to_string(&pick.arguments).unwrap_or_default();
    tracing::info!(
        server = %pick.server,
        tool = %pick.tool,
        args_len = args_str.len(),
        "calling MCP tool"
    );

    let fut = async {
        let (transport, _child) = friday_mcp::transport::stdio_transport_for_with_env(
            &server.command,
            &server.args,
            &server.env,
        )
        .map_err(|e| format!("failed to launch MCP server '{}': {e}", server.name))?;

        let mut client = friday_mcp::protocol::McpClient::new(transport);
        client
            .initialize()
            .await
            .map_err(|e| format!("MCP initialize failed for '{}': {e}", server.name))?;

        let result = client
            .call_tool(&pick.tool, pick.arguments.clone())
            .await
            .map_err(|e| format!("MCP tool call failed: {e}"))?;

        let mut texts = Vec::new();
        for content in &result.content {
            if let Some(t) = content.get("text").and_then(|s| s.as_str()) {
                texts.push(t.to_string());
            } else if let Some(s) = content.as_str() {
                texts.push(s.to_string());
            } else {
                texts.push(content.to_string());
            }
        }
        let full = texts.join("\n");
        if result.is_error {
            let err_msg = if full.is_empty() {
                "MCP tool returned an error".to_string()
            } else {
                full
            };
            return Err(err_msg);
        }
        let capped: String = full.chars().take(8000).collect();
        Ok(capped)
    };

    let outcome = match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(res) => res,
        Err(_) => Err(format!(
            "tool call to '{}' timed out after 30s",
            pick.server
        )),
    };

    match &outcome {
        Ok(res) => {
            tracing::info!(
                server = %pick.server,
                tool = %pick.tool,
                result_len = res.len(),
                "MCP tool call succeeded"
            );
        }
        Err(e) => {
            tracing::warn!(
                server = %pick.server,
                tool = %pick.tool,
                error_len = e.len(),
                "MCP tool call failed"
            );
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_config_valid() {
        let json = r#"{
            "mcpServers": {
                "git": {
                    "command": "uvx",
                    "args": ["mcp-server-git"],
                    "env": {"FOO": "bar"}
                },
                "weather": {
                    "command": "weather-server"
                }
            }
        }"#;
        let (servers, err) = parse_config(json);
        assert!(err.is_none());
        assert_eq!(servers.len(), 2);
        let git = &servers["git"];
        assert_eq!(git.command, "uvx");
        assert_eq!(git.args, vec!["mcp-server-git"]);
        assert_eq!(git.env.get("FOO").unwrap(), "bar");

        let weather = &servers["weather"];
        assert_eq!(weather.command, "weather-server");
        assert!(weather.args.is_empty());
        assert!(weather.env.is_empty());
    }

    #[test]
    fn parse_config_bad_json() {
        let bad = "{ mcpServers: invalid json";
        let (servers, err) = parse_config(bad);
        assert!(servers.is_empty());
        assert!(err.is_some());
    }

    #[test]
    fn parse_config_empty() {
        let (servers, err) = parse_config("");
        assert!(servers.is_empty());
        assert!(err.is_none());
    }

    #[test]
    fn list_servers_on_and_off() {
        let cfg = r#"{
            "mcpServers": {
                "alpha": {"command": "a"},
                "beta": {"command": "b"},
                "gamma": {"command": "c"}
            }
        }"#;
        let on = "alpha\ngamma\n";
        let list = list_servers(cfg, on);
        assert_eq!(
            list,
            vec![
                ServerStatus {
                    name: "alpha".to_string(),
                    on: true,
                },
                ServerStatus {
                    name: "beta".to_string(),
                    on: false,
                },
                ServerStatus {
                    name: "gamma".to_string(),
                    on: true,
                },
            ]
        );
    }

    #[test]
    fn parse_pick_none() {
        let tools = vec![ToolInfo {
            server: "git".to_string(),
            name: "status".to_string(),
            description: "git status".to_string(),
            input_schema: json!({}),
        }];
        assert_eq!(parse_pick("NONE", &tools), None);
        assert_eq!(parse_pick("none", &tools), None);
        assert_eq!(parse_pick("  NONE.  ", &tools), None);
    }

    #[test]
    fn parse_pick_junk() {
        let tools = vec![ToolInfo {
            server: "git".to_string(),
            name: "status".to_string(),
            description: "git status".to_string(),
            input_schema: json!({}),
        }];
        assert_eq!(parse_pick("I'm not sure what tool to use", &tools), None);
        assert_eq!(parse_pick(r#"{"random": "json"}"#, &tools), None);
        assert_eq!(
            parse_pick(
                r#"{"server": "git", "tool": "status", "arguments": "not-an-object"}"#,
                &tools
            ),
            None
        );
    }

    #[test]
    fn parse_pick_unlisted_tool() {
        let tools = vec![ToolInfo {
            server: "git".to_string(),
            name: "status".to_string(),
            description: "git status".to_string(),
            input_schema: json!({}),
        }];
        let reply = r#"{"server": "git", "tool": "commit", "arguments": {}}"#;
        assert_eq!(parse_pick(reply, &tools), None);

        let reply2 = r#"{"server": "other", "tool": "status", "arguments": {}}"#;
        assert_eq!(parse_pick(reply2, &tools), None);
    }

    #[test]
    fn parse_pick_code_fences_and_success() {
        let tools = vec![ToolInfo {
            server: "git".to_string(),
            name: "status".to_string(),
            description: "git status".to_string(),
            input_schema: json!({}),
        }];
        let reply = "Here is the tool pick:\n```json\n{\n  \"server\": \"git\",\n  \"tool\": \"status\",\n  \"arguments\": {\"repo\": \".\"}\n}\n```\nHope this helps!";
        let pick = parse_pick(reply, &tools).expect("should parse pick in code fences");
        assert_eq!(pick.server, "git");
        assert_eq!(pick.tool, "status");
        assert_eq!(pick.arguments, json!({"repo": "."}));
    }

    #[test]
    fn a_confirmed_call_is_read_back_exactly() {
        let tools = vec![ToolInfo {
            server: "files".into(),
            name: "read".into(),
            description: String::new(),
            input_schema: serde_json::json!({}),
        }];
        let pick = parse_confirmed("files.read {\"path\":\"a.txt\"}", &tools).unwrap();
        assert_eq!(pick.tool, "read");
        assert_eq!(pick.arguments, serde_json::json!({"path": "a.txt"}));
        assert!(parse_confirmed("files.write {}", &tools).is_none());
        assert!(parse_confirmed("files.read not-json", &tools).is_none());
        assert!(parse_confirmed("rm -rf /", &tools).is_none());
        // "db" + "table.get" and "db.table" + "get" read the same: neither runs.
        let two = |server: &str, name: &str| ToolInfo {
            server: server.into(),
            name: name.into(),
            description: String::new(),
            input_schema: serde_json::json!({}),
        };
        let clash = vec![two("db", "table.get"), two("db.table", "get")];
        assert!(parse_confirmed("db.table.get {}", &clash).is_none());
    }

    /// A real server over stdio: a few lines of Python speaking MCP's
    /// JSON-RPC (initialize, tools/list, tools/call), with its env passed.
    #[tokio::test]
    async fn a_real_server_lists_its_tools_and_answers_a_call() {
        if std::process::Command::new("python3")
            .arg("-V")
            .output()
            .is_err()
        {
            return;
        }
        let script = r#"
import json, os, sys
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m:
        continue
    if m["method"] == "initialize":
        r = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "t", "version": "1"}}
    elif m["method"] == "tools/list":
        r = {"tools": [{"name": "greet", "description": "Says hello", "inputSchema": {"type": "object"}}]}
    else:
        who = m["params"]["arguments"].get("who", "?")
        r = {"content": [{"type": "text", "text": "hello " + who + " " + os.environ.get("GREETING_SUFFIX", "")}]}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
"#;
        let server = ServerConfig {
            name: "t".into(),
            command: "python3".into(),
            args: vec!["-c".into(), script.into()],
            env: HashMap::from([("GREETING_SUFFIX".to_string(), "!".to_string())]),
        };
        let tools = connect(&server).await.expect("connects");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "greet");
        let pick = Pick {
            server: "t".into(),
            tool: "greet".into(),
            arguments: serde_json::json!({"who": "FRIDAY"}),
        };
        assert_eq!(
            call_on(&server, &pick).await.unwrap().trim(),
            "hello FRIDAY !"
        );
    }
}
