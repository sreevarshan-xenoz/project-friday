//! FRIDAY as an MCP server (`friday-ctl mcp`, over stdio): other AI apps can
//! read the person's kept thoughts and FRIDAY's timeline, and ask FRIDAY a
//! question. Each tool is off until the person allows it (`friday mcp allow
//! <tool>`, one name per line in `mcp-allow` in FRIDAY's config folder); a
//! call to one that isn't allowed says how to allow it and does nothing.

use std::path::PathBuf;

use serde_json::{json, Value};

/// The tools, with what the calling app is told about each.
pub const TOOLS: &[(&str, &str)] = &[
    (
        "friday_thoughts",
        "The ideas, decisions, actions and open questions the person kept, newest last. \
         Optional argument project: a project name or words from it.",
    ),
    (
        "friday_timeline",
        "What happened on this computer, from FRIDAY's own logs (asks, answers, actions). \
         Optional argument when: today, yesterday or week.",
    ),
    (
        "friday_ask",
        "Ask FRIDAY a question: it answers with the person's own AI provider and what it knows \
         about their work. Anything that would ask the person first (running a command, \
         changing a file, opening a page, moving windows) doesn't run from here. \
         Argument question.",
    ),
];

/// The line that marks a goal as sent by another app over MCP: like text
/// quoted from a page, it may carry instructions that aren't the person's,
/// so every action that can ask first does (and, with nobody there to say
/// yes, doesn't run).
pub const FROM_ANOTHER_APP: &str = "[asked by another app]";

/// `question` as the goal FRIDAY runs for a connected app.
pub fn from_another_app(question: &str) -> String {
    format!(
        "{question}\n\n{FROM_ANOTHER_APP}\nAn app connected to FRIDAY over MCP sent this; it isn't the person's own request."
    )
}

/// `mcp-allow` in FRIDAY's config folder (`$XDG_CONFIG_HOME/friday`, or
/// `%APPDATA%\friday` on Windows).
pub fn allow_file() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_default().join("friday").join("mcp-allow")
}

/// Whether the person allowed `tool` (its name on a line of `allow`).
pub fn allowed(allow: &str, tool: &str) -> bool {
    allow.lines().any(|l| l.trim() == tool)
}

fn tool_list(allow: &str) -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|(name, about)| {
            let (key, required) = match *name {
                "friday_thoughts" => ("project", false),
                "friday_timeline" => ("when", false),
                _ => ("question", true),
            };
            let state = if allowed(allow, name) {
                "Allowed by the person."
            } else {
                "Not allowed yet: the person can allow it with `friday mcp allow` and its name."
            };
            json!({
                "name": name,
                "description": format!("{about} {state}"),
                "inputSchema": {
                    "type": "object",
                    "properties": { key: { "type": "string" } },
                    "required": if required { vec![key] } else { vec![] },
                },
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// The answer to one JSON-RPC message, or None for a notification. `run`
/// does an allowed tool's work: its text, or what went wrong.
pub fn handle(
    msg: &Value,
    allow: &str,
    run: &dyn Fn(&str, &Value) -> Result<String, String>,
) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "friday", "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "tools/list" => tool_list(allow),
        "tools/call" => {
            let name = msg
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = msg
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(json!({}));
            let (text, error) = if !TOOLS.iter().any(|(n, _)| *n == name) {
                (format!("FRIDAY has no tool called {name}."), true)
            } else if !allowed(allow, name) {
                (
                    format!(
                        "The person hasn't allowed {name}. They can run: friday mcp allow {name}"
                    ),
                    true,
                )
            } else {
                match run(name, &args) {
                    Ok(text) => (text, false),
                    Err(e) => (e, true),
                }
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": error })
        }
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("no method {method}") },
            }))
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(_: &str, args: &Value) -> Result<String, String> {
        Ok(format!("ran with {args}"))
    }

    #[test]
    fn it_introduces_itself_and_lists_its_tools() {
        let init = handle(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
            "",
            &ok,
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "friday");
        assert!(handle(
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            "",
            &ok
        )
        .is_none());
        let list = handle(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            "friday_ask\n",
            &ok,
        )
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        let ask = tools.iter().find(|t| t["name"] == "friday_ask").unwrap();
        assert!(ask["description"]
            .as_str()
            .unwrap()
            .contains("Allowed by the person."));
        assert_eq!(ask["inputSchema"]["required"], json!(["question"]));
        let thoughts = tools
            .iter()
            .find(|t| t["name"] == "friday_thoughts")
            .unwrap();
        assert!(thoughts["description"]
            .as_str()
            .unwrap()
            .contains("Not allowed yet"));
    }

    #[test]
    fn a_tool_runs_only_once_the_person_allowed_it() {
        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "friday_thoughts", "arguments": {"project": "site"}}});
        let refused = handle(&call, "friday_ask\n", &|_, _| {
            panic!("ran a tool that isn't allowed")
        })
        .unwrap();
        assert_eq!(refused["result"]["isError"], true);
        assert!(refused["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("friday mcp allow friday_thoughts"));
        let ran = handle(&call, "friday_thoughts\n", &ok).unwrap();
        assert_eq!(ran["result"]["isError"], false);
        assert_eq!(
            ran["result"]["content"][0]["text"],
            r#"ran with {"project":"site"}"#
        );
        let unknown =
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "rm_rf"}});
        assert_eq!(
            handle(&unknown, "rm_rf\n", &ok).unwrap()["result"]["isError"],
            true
        );
        let bad = handle(
            &json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"}),
            "",
            &ok,
        )
        .unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }
}
