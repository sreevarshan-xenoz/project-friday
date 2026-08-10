//! Live-proof demo for the Browser Organ (`specs/browser-organ.md` §7).
//!
//! Launches Chrome (headed) with a **dedicated FRIDAY profile**
//! (`FRIDAY_BROWSER_PROFILE`, default `~/.friday/browser/`), opens chatgpt.com,
//! sends a message through the [`ChatGPTAdapter`] WebAgent, waits for the reply,
//! and prints it.
//!
//! Before the first run: log into chatgpt.com in the dedicated profile once
//! (run with `FRIDAY_BROWSER_HEADED=1` and open the profile's Chrome manually,
//! or just log in during the demo run). FRIDAY never types credentials — the
//! profile *is* the login.
//!
//! ```powershell
//! $env:FRIDAY_BROWSER_HEADED = "1"
//! cargo run -p friday-actions --example webagent_demo -- "Reply with exactly: OK"
//! ```
//!
//! Exit code 1 with a clear message if chatgpt.com isn't reachable / logged in.

use friday_actions::{agent_for_site, BrowserAdapter};
use std::path::PathBuf;

fn default_profile_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".friday").join("browser")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Reply with exactly: OK".to_string());

    let profile = std::env::var("FRIDAY_BROWSER_PROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_profile_dir());
    std::fs::create_dir_all(&profile)?;
    println!("[webagent] dedicated profile: {}", profile.display());

    let browser = BrowserAdapter::new().with_user_data_dir(profile);
    let agent = agent_for_site("https://chatgpt.com", browser);
    println!("[webagent] site adapter: {}", agent.site());

    println!("[webagent] opening chatgpt.com …");
    agent
        .open("https://chatgpt.com")
        .await
        .map_err(|e| format!("open failed: {e}"))?;
    println!("[webagent] page open.");

    println!("[webagent] sending: {message:?}");
    agent
        .send_message(&message)
        .await
        .map_err(|e| format!("send failed — {e}"))?;
    println!("[webagent] sent, waiting for the reply to finish generating …");

    let resp = agent
        .read_response()
        .await
        .map_err(|e| format!("read failed — {e}"))?;

    println!();
    println!("=== ChatGPT reply ===");
    println!("{}", resp.text);
    println!("======================");
    println!("[webagent] title={:?} url={}", resp.title, resp.url);
    Ok(())
}
