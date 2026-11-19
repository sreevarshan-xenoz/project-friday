//! FRIDAY in opencode sessions the person started themselves. opencode's
//! sessions run in its background service, which announces each permission
//! it asks for (`permission.asked`) on an event stream. FRIDAY follows that
//! stream and answers "once" when its rules allow the step; anything else is
//! left to opencode's own prompt, with a notification. FRIDAY never answers
//! "always".
//!
//! The service's address and password are in opencode's `service.json`; the
//! password is used for these requests only and never logged. On unless the
//! person turned FRIDAY off in their own sessions (`friday agents hooks
//! off`, `FRIDAY_AGENT_HOOKS=0`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde_json::{json, Value};

use crate::agent_hooks::{decide_session_step, sessions_off_marker};
use crate::agent_policy::{Decision, Kind, Step};

/// The step a `permission.asked` event asks about.
pub fn step_from_opencode(action: &str, resources: &[String]) -> Step {
    let kind = match action {
        "shell" | "bash" => Kind::Execute,
        "edit" | "write" | "patch" | "multiedit" => Kind::Edit,
        "read" => Kind::Read,
        "glob" | "grep" | "list" | "codesearch" => Kind::Search,
        "webfetch" | "websearch" => Kind::Fetch,
        _ => Kind::Other,
    };
    Step {
        kind,
        command: (kind == Kind::Execute).then(|| resources.join(" ")),
        paths: if kind == Kind::Execute {
            Vec::new()
        } else {
            resources.to_vec()
        },
    }
}

/// opencode's service settings file: `$XDG_CONFIG_HOME/opencode/service.json`.
fn service_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(config.join("opencode").join("service.json"))
}

/// The service's address and password, when opencode has a service.
pub fn service(file: &Path) -> Option<(String, String)> {
    let doc: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let port = doc["port"].as_u64()?;
    let password = doc["password"].as_str()?.to_string();
    Some((format!("http://127.0.0.1:{port}"), password))
}

/// What FRIDAY does about one `permission.asked` event: the reply to send,
/// if any, as (session id, request id, folder).
pub fn handle(event: &Value, log_dir: &Path) -> Option<(String, String, String)> {
    let data = &event["data"];
    let folder = event["location"]["directory"].as_str().unwrap_or("");
    let resources: Vec<String> = data["resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect();
    let step = step_from_opencode(data["action"].as_str().unwrap_or(""), &resources);
    match decide_session_step("opencode", &step, Path::new(folder), false, log_dir) {
        Decision::Allow => Some((
            data["sessionID"].as_str()?.to_string(),
            data["id"].as_str()?.to_string(),
            folder.to_string(),
        )),
        Decision::Ask(_) => None,
    }
}

/// Follows one connection to the service until it ends.
pub async fn follow(base: &str, password: &str, log_dir: &Path) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base}/api/event"))
        .basic_auth("opencode", Some(password))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("opencode's service said {}", response.status()));
    }
    let mut stream = response.bytes_stream();
    // Bytes until a whole line is in: a character can be split across chunks.
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let bytes: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&bytes);
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            if event["type"] != "permission.asked" || sessions_off_marker().exists() {
                continue;
            }
            if let Some((session, request, folder)) = handle(&event, log_dir) {
                let reply = client
                    .post(format!(
                        "{base}/api/session/{session}/permission/{request}/reply"
                    ))
                    .query(&[("location[directory]", folder.as_str())])
                    .basic_auth("opencode", Some(password))
                    .json(&json!({"decision": "once"}))
                    .send()
                    .await;
                if let Err(e) = reply {
                    tracing::warn!("opencode: couldn't answer a permission: {e}");
                }
            }
        }
    }
    Ok(())
}

/// Follows opencode's service for as long as the daemon runs, reconnecting
/// when the service restarts or appears.
pub async fn watch(log_dir: PathBuf) {
    loop {
        let found = service_file().and_then(|f| service(&f));
        if let (Some((base, password)), true) = (found, crate::agent_hooks::enabled()) {
            if let Err(e) = follow(&base, &password, &log_dir).await {
                tracing::debug!("opencode: {e}");
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_actions_become_steps() {
        let s = step_from_opencode("shell", &["cargo test".into()]);
        assert_eq!(
            (s.kind, s.command.as_deref()),
            (Kind::Execute, Some("cargo test"))
        );
        let e = step_from_opencode("edit", &["notes.txt".into()]);
        assert_eq!(
            (e.kind, e.paths),
            (Kind::Edit, vec!["notes.txt".to_string()])
        );
        assert_eq!(
            step_from_opencode("external_directory", &[]).kind,
            Kind::Other
        );
    }

    #[cfg(unix)]
    #[test]
    fn safe_steps_get_a_reply_and_risky_ones_none() {
        let base = std::env::temp_dir().join(format!("friday-opencode-{}", std::process::id()));
        let (logs, site) = (base.join("logs"), base.join("code").join("site"));
        std::fs::create_dir_all(&site).unwrap();
        let event = |action: &str, resource: &str| {
            json!({"type": "permission.asked", "location": {"directory": site},
                "data": {"id": "per_1", "sessionID": "ses_1", "action": action, "resources": [resource]}})
        };
        let reply = handle(&event("shell", "git status"), &logs).unwrap();
        assert_eq!((reply.0.as_str(), reply.1.as_str()), ("ses_1", "per_1"));
        assert!(handle(&event("shell", "rm -rf build"), &logs).is_none());
        assert!(handle(&event("edit", "package.json"), &logs).is_none());
        assert!(handle(&event("edit", "src/app.ts"), &logs).is_some());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_service_file_gives_an_address_and_password() {
        let f = std::env::temp_dir().join(format!("friday-oc-service-{}.json", std::process::id()));
        std::fs::write(&f, r#"{"port": 49391, "password": "example-only"}"#).unwrap();
        assert_eq!(
            service(&f),
            Some(("http://127.0.0.1:49391".into(), "example-only".into()))
        );
        let _ = std::fs::remove_file(f);
    }

    /// Against a real opencode service: `FRIDAY_TEST_OPENCODE_SERVICE` names
    /// its service.json; the test follows the stream for a while and answers
    /// whatever it sees. Run it while a session asks for something.
    #[tokio::test]
    #[ignore = "follows a live opencode service"]
    async fn follows_a_live_service() {
        let Ok(file) = std::env::var("FRIDAY_TEST_OPENCODE_SERVICE") else {
            return;
        };
        let (base, password) = service(Path::new(&file)).expect("service.json");
        let logs = std::env::temp_dir().join("friday-opencode-live");
        let _ =
            tokio::time::timeout(Duration::from_secs(60), follow(&base, &password, &logs)).await;
        eprintln!(
            "{}",
            std::fs::read_to_string(logs.join("hooks.jsonl")).unwrap_or_default()
        );
    }
}
