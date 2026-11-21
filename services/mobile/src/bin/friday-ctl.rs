//! friday-ctl — the shell's one-command interface to the FRIDAY Linux daemon.
//!
//! This is what `friday.shell` (the Quickshell plugin) calls to issue a
//! controlled command from the desktop, and what a human can run from a
//! terminal. It reads the daemon's session token + listen address from the
//! file-backed `state.json` (the same contract the plugin watches), connects
//! to the loopback WebSocket ingress, and sends one `planner.goal` command.
//!
//! Usage:
//!   friday-ctl goal "List the files in this folder"
//!   friday-ctl goal --wait "open chromium"   (block until the goal finishes;
//!                                            exit status = its outcome)
//!   friday-ctl goal --wait --stream "..."    (meanwhile print the answer so
//!                                            far: {"partial": ...} lines;
//!                                            also for context screen)
//!   friday-ctl system-info
//!   friday-ctl say "text"              (speak it with FRIDAY's voice on this
//!                                      computer; "-" reads the text from stdin;
//!                                      a new say replaces the one speaking)
//!   friday-ctl say --stop              (stop speaking)
//!   friday-ctl say --setup [kitten]    (download and install the voice, checked
//!                                      against pinned checksums)
//!   friday-ctl cancel                  (cancel the goal in flight)
//!   friday-ctl agent-hook claude|agy   (Claude Code's / agy's permission hook)
//!   friday-ctl agent-hooks on|off|status (FRIDAY in your own agent sessions)
//!   friday-ctl context screen          (interactive consent)
//!   friday-ctl context screen --yes    (consent already collected by the shell panel)
//!   friday-ctl context screen --yes --ask "what does this error mean?"
//!                                      (answer a question about the screen)
//!
//! The daemon's address comes from `state.json` (`listen`). `FRIDAY_CTL_ADDR`
//! and `FRIDAY_CTL_TOKEN` override the state-file values for
//! scripting/testing. `FRIDAY_CONTEXT_PROVIDER` names the provider a `context
//! screen` share goes to; unset, the daemon routes it to a vision model (the
//! user's provider before keyless OpenCode Zen).

use std::time::Duration;

use friday_mobile::protocol::{decode_server, ClientMessage, ServerMessage};
use friday_mobile::shell_state::ShellState;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// The disclosure text the terminal chooser must show **before** transmission.
/// Pulled from the frozen ContextGate so the client never duplicates the
/// system-of-record wording. A remote provider disclosure is always present.
fn screen_disclosure() -> String {
    friday_intelligence::ContextGate::new()
        .disclosure_for(
            friday_intelligence::ContextChannel::Screen,
            friday_intelligence::ProviderTarget::Subscription,
        )
        .unwrap_or("This sends a screen capture to a remote provider.")
        .to_string()
}

/// R5-2-5-3: the terminal reference implementation of the consent UX. It
/// explains what would be shared and to whom, shows data-use disclosure, then
/// demands an explicit `y` before anything is sent. Declining returns `false`
/// so the caller can abort before ever contacting the daemon — the provider is
/// guaranteed to see nothing.
fn confirm_context_consent() -> bool {
    let provider = std::env::var("FRIDAY_CONTEXT_PROVIDER")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "your AI provider's vision model".to_string());

    println!("FRIDAY — share the current Screen with the assistant");
    println!();
    println!("What would be shared:");
    println!("  a screen capture of the current display (a still image)");
    println!();
    println!("Provider: {provider}");
    println!("Data use: {}", screen_disclosure());
    println!();
    print!("Share this screen? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    answer.trim().eq_ignore_ascii_case("y")
}

/// `friday-ctl say`: no daemon needed, the voice runs here.
/// `friday-ctl watch kitty <socket> <id> <command> | region <geometry> | stop`
/// (see `friday_mobile::watch`). Runs until it has something to say, is
/// replaced or stopped, or two hours pass.
fn watch(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::watch::{self as w, Kitty, Region, RegionEvent};
    use std::process::{Command, ExitCode, Stdio};
    let pidfile = std::path::PathBuf::from(
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()),
    )
    .join("friday-watch.pid");
    // The watch running now, if any (the pid file names a live process).
    let running = std::fs::read_to_string(&pidfile)
        .ok()
        .and_then(|p| p.trim().parse::<u32>().ok())
        // A pid can be reused after a reboot or a crash: only a process run
        // as `friday-ctl watch …` counts.
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| {
                let mut argv = c.split(|b| *b == 0);
                let exe = argv.next().unwrap_or_default();
                exe.rsplit(|b| *b == b'/').next() == Some(b"friday-ctl".as_slice())
                    && argv.next() == Some(b"watch".as_slice())
            })
        });
    let stop_running = || {
        if let Some(pid) = running {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        let _ = std::fs::remove_file(&pidfile);
    };
    let notify = |title: &str, body: &str| {
        let _ = Command::new("notify-send")
            .args(["-a", "FRIDAY", "--", title, body])
            .stderr(Stdio::null())
            .status();
    };
    let usage = || {
        eprintln!("usage: friday-ctl watch kitty <socket> <window id> <command> | region <X,Y WxH> | stop");
        ExitCode::from(2)
    };
    match args.first().map(String::as_str) {
        Some("stop") => {
            println!(
                "{}",
                if running.is_some() {
                    "Stopped watching."
                } else {
                    "Nothing is being watched."
                }
            );
            stop_running();
            return ExitCode::SUCCESS;
        }
        Some("kitty") if args.len() >= 4 && args[2].parse::<u64>().is_ok() => {}
        Some("region") if args.len() == 2 && w::valid_geometry(&args[1]) => {}
        _ => return usage(),
    }
    stop_running();
    let _ = std::fs::write(&pidfile, std::process::id().to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(w::MAX_SECS);
    let mut region = Region::default();
    let me = std::process::id().to_string();
    let mut blind = 0;
    while std::time::Instant::now() < deadline {
        // Replaced by a newer watch (or stopped): this one ends quietly.
        if std::fs::read_to_string(&pidfile).ok().as_deref() != Some(me.as_str()) {
            return ExitCode::SUCCESS;
        }
        if args[0] == "kitty" {
            let ls = Command::new("timeout")
                .args([
                    "5",
                    "kitten",
                    "@",
                    "--to",
                    &format!("unix:{}", args[1]),
                    "ls",
                ])
                .stderr(Stdio::null())
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            match w::kitty_state(&ls, args[2].parse().unwrap_or(0)) {
                Kitty::Running => {}
                Kitty::Finished(status) => {
                    let (title, body) = w::finished_message(&args[3..].join(" "), status);
                    notify(&title, &body);
                    break;
                }
                Kitty::Gone => {
                    notify("FRIDAY", "The window you asked me to watch was closed.");
                    break;
                }
            }
        } else if let Some(frame) = Command::new("timeout")
            .args(["5", "grim", "-g", &args[1], "-t", "ppm", "-"])
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| o.stdout)
            .filter(|f| w::parse_ppm(f).is_some())
        {
            blind = 0;
            match region.look(frame) {
                Some(RegionEvent::Changed) => {
                    notify("FRIDAY", "The area you're watching changed.");
                    break;
                }
                Some(RegionEvent::Stopped) => {
                    notify("FRIDAY", "The area you're watching has stopped changing.");
                    break;
                }
                None => {}
            }
        } else if args[0] == "region" {
            // No picture (the screen is locked, or grim failed): a minute of
            // that and the watch ends, saying why.
            blind += 1;
            if blind >= 30 {
                notify(
                    "FRIDAY",
                    "I can't see that area any more, so I stopped watching it.",
                );
                break;
            }
        }
        std::thread::sleep(Duration::from_secs(w::EVERY_SECS));
    }
    // Only our own pid file: a newer watch may have replaced it.
    if std::fs::read_to_string(&pidfile).ok().as_deref() == Some(me.as_str()) {
        let _ = std::fs::remove_file(&pidfile);
    }
    ExitCode::SUCCESS
}

/// `friday-ctl recall watch | search <question>` (see `friday_mobile::recall`).
async fn recall(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::context_read::ScreenText;
    use friday_mobile::recall::{self, Seen};
    let path = recall::file();
    let now = || chrono::Utc::now().timestamp();
    match args.first().map(String::as_str) {
        Some("search") => {
            let found = recall::search(&path, now(), &args[1..].join(" "));
            print!("{}", friday_mobile::local_docs::render(&found));
            std::process::ExitCode::SUCCESS
        }
        Some("watch") => {
            let Some(reader) = friday_mobile::agent_desktop::detect().await else {
                eprintln!("friday-ctl: recall needs agent-desktop to read windows");
                return std::process::ExitCode::FAILURE;
            };
            let own = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
                })
                .unwrap_or_default()
                .join("friday/private-windows");
            loop {
                let win: serde_json::Value = std::process::Command::new("hyprctl")
                    .args(["activewindow", "-j"])
                    .output()
                    .ok()
                    .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                    .unwrap_or_default();
                let app = win["class"].as_str().unwrap_or_default();
                let title = win["title"].as_str().unwrap_or_default();
                let own_list = std::fs::read_to_string(&own).unwrap_or_default();
                // The reader takes the whole app, every window of it: one
                // private window (a private Firefox one) keeps it all out.
                let clients: serde_json::Value = std::process::Command::new("hyprctl")
                    .args(["clients", "-j"])
                    .output()
                    .ok()
                    .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                    .unwrap_or_default();
                let any_private = clients.as_array().into_iter().flatten().any(|c| {
                    c["class"].as_str() == Some(app)
                        && recall::private(app, c["title"].as_str().unwrap_or_default(), &own_list)
                });
                if !app.is_empty() && !any_private && !recall::private(app, title, &own_list) {
                    if let Some(text) = reader.window_text(app).await {
                        let _ = recall::add(
                            &path,
                            Seen {
                                ts: now(),
                                app: app.to_string(),
                                title: title.to_string(),
                                text,
                            },
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(recall::EVERY_SECS)).await;
            }
        }
        _ => {
            eprintln!("usage: friday-ctl recall watch | search <question>");
            std::process::ExitCode::from(2)
        }
    }
}

/// `friday-ctl routine save|run|list|rm [<name>]` (see
/// `friday_mobile::routine`).
async fn routine(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::routine::{self as r, Routine, Step};
    use std::process::ExitCode;
    let dir = r::dir();
    let name = args[1.min(args.len())..].join(" ");
    let named = |what: &str| -> Result<(), ExitCode> {
        if r::valid_name(&name) {
            return Ok(());
        }
        eprintln!(
            "friday-ctl: {what} needs a name made of letters, digits, spaces, - or _ (up to 40)"
        );
        Err(ExitCode::from(2))
    };
    // `friday routine record`: Hyprland's events while you work, kept in the
    // runtime folder until `finish <name>` turns them into a routine.
    let recording = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("friday-recording.txt");
    match args.first().map(String::as_str) {
        #[cfg(unix)]
        Some("record") => {
            use std::io::{BufRead, Write};
            let socket = std::env::var_os("XDG_RUNTIME_DIR")
                .zip(std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE"))
                .map(|(run, sig)| {
                    std::path::PathBuf::from(run)
                        .join("hypr")
                        .join(sig)
                        .join(".socket2.sock")
                });
            let Some(Ok(stream)) = socket.map(std::os::unix::net::UnixStream::connect) else {
                eprintln!("friday-ctl: recording a routine needs Hyprland");
                return ExitCode::FAILURE;
            };
            let clients: serde_json::Value = std::process::Command::new("hyprctl")
                .args(["-j", "clients"])
                .output()
                .ok()
                .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                .unwrap_or_default();
            let open: Vec<(String, String)> = clients
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    (
                        c["address"].as_str().unwrap_or("").to_string(),
                        c["class"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let Ok(mut file) = opts.open(&recording) else {
                eprintln!("friday-ctl: couldn't write {}", recording.display());
                return ExitCode::FAILURE;
            };
            let _ = writeln!(
                file,
                "#open {}",
                serde_json::to_string(&open).unwrap_or_default()
            );
            // Its own pid, for `friday routine stop` (a wrapper's would be wrong).
            let _ = std::fs::write(
                recording.with_extension("pid"),
                std::process::id().to_string(),
            );
            for line in std::io::BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                // Only the events a routine can do again; never window titles.
                if let Some((event, data)) = line.split_once(">>") {
                    let keep = match event {
                        "openwindow" => data.splitn(4, ',').take(3).collect::<Vec<_>>().join(","),
                        "movewindowv2" | "workspacev2" => data.to_string(),
                        _ => continue,
                    };
                    let _ = writeln!(file, "{event}>>{keep}");
                }
            }
            ExitCode::SUCCESS
        }
        Some("finish") => {
            if let Err(c) = named("finish") {
                return c;
            }
            let text = std::fs::read_to_string(&recording).unwrap_or_default();
            let open: Vec<(String, String)> = text
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("#open "))
                .and_then(|j| serde_json::from_str(j).ok())
                .unwrap_or_default();
            let steps = r::from_events(&text, &open);
            let _ = std::fs::remove_file(&recording);
            if steps.is_empty() {
                eprintln!("Nothing to save: open an app, move a window or switch workspaces while recording.");
                return ExitCode::FAILURE;
            }
            let routine = Routine {
                name: name.trim().to_string(),
                goal: "recorded by you".into(),
                steps,
            };
            if let Err(e) = r::save(&dir, &routine) {
                eprintln!("friday-ctl: couldn't save it: {e}");
                return ExitCode::FAILURE;
            }
            println!("Saved \"{}\": {} steps.", routine.name, routine.steps.len());
            for s in &routine.steps {
                println!("  {} {}", s.verb, s.params);
            }
            ExitCode::SUCCESS
        }
        Some("list") => {
            for x in r::list(&dir) {
                println!("{}\t{} steps\t{}", x.name, x.steps.len(), x.goal);
            }
            ExitCode::SUCCESS
        }
        Some("rm") => {
            if let Err(c) = named("rm") {
                return c;
            }
            if r::remove(&dir, &name) {
                println!("Removed the routine \"{name}\".");
                ExitCode::SUCCESS
            } else {
                eprintln!("There's no routine called \"{name}\".");
                ExitCode::FAILURE
            }
        }
        Some("save") => {
            if let Err(c) = named("save") {
                return c;
            }
            let log = friday_mobile::desktop_tool::audit_dir()
                .and_then(|d| std::fs::read_to_string(d.join("actions.jsonl")).ok())
                .unwrap_or_default();
            let Some(rec) = r::last_task(&log) else {
                eprintln!("FRIDAY hasn't done anything on your desktop to save yet. Ask it to (for example \"put Firefox on workspace 2\"), then save it.");
                return ExitCode::FAILURE;
            };
            let routine = Routine {
                name: name.trim().to_string(),
                goal: rec.goal,
                steps: rec.steps,
            };
            if let Err(e) = r::save(&dir, &routine) {
                eprintln!("friday-ctl: couldn't save it: {e}");
                return ExitCode::FAILURE;
            }
            println!(
                "Saved \"{}\": {} steps, from \"{}\".",
                routine.name,
                routine.steps.len(),
                routine.goal
            );
            if rec.left_out > 0 {
                println!(
                    "{} step(s) were left out (they needed your confirmation, or typed text, which isn't saved); do those yourself.",
                    rec.left_out
                );
            }
            ExitCode::SUCCESS
        }
        Some("run") => {
            if let Err(c) = named("run") {
                return c;
            }
            let Some(routine) = r::load(&dir, &name) else {
                eprintln!("There's no routine called \"{name}\".");
                return ExitCode::FAILURE;
            };
            struct Live {
                desktop: friday_actions::DesktopAdapter,
                #[cfg(target_os = "linux")]
                ui: friday_actions::UiAdapter,
            }
            #[async_trait::async_trait]
            impl r::Runner for Live {
                async fn run(&mut self, step: &Step) -> Result<(), String> {
                    use friday_actions::{ActionAdapter, ActionRequest};
                    let res = match step.verb.strip_prefix("ui_") {
                        #[cfg(target_os = "linux")]
                        Some(v) => {
                            // A click is found again by the label it had.
                            let mut p = step.params.clone();
                            if v == "click" {
                                p = serde_json::json!({
                                    "app": step.params["app"],
                                    "name": step.params["label"],
                                });
                            }
                            self.ui.execute(ActionRequest::new("ui", v, p)).await
                        }
                        #[cfg(not(target_os = "linux"))]
                        Some(_) => friday_actions::ActionResult::error("not on this system yet"),
                        None => {
                            self.desktop
                                .execute(ActionRequest::new(
                                    "desktop",
                                    &step.verb,
                                    step.params.clone(),
                                ))
                                .await
                        }
                    };
                    if res.success {
                        Ok(())
                    } else {
                        Err(res.error.unwrap_or_else(|| "failed".into()))
                    }
                }
            }
            // Destructive verbs ask each time: refuse them here too, in case
            // a file was edited by hand.
            if let Some(bad) = routine.steps.iter().find(|s| {
                friday_actions::desktop_risk_of(&s.verb)
                    == Some(friday_actions::DesktopRisk::Destructive)
            }) {
                eprintln!("The routine \"{}\" has a step ({}) that needs your confirmation each time, so it isn't run.", routine.name, bad.verb);
                return ExitCode::FAILURE;
            }
            let mut live = Live {
                desktop: friday_actions::DesktopAdapter::new(),
                #[cfg(target_os = "linux")]
                ui: friday_actions::UiAdapter::new(),
            };
            match r::replay(&routine, &mut live).await {
                Ok(n) => {
                    println!("Ran \"{}\": {n} steps.", routine.name);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("\"{}\" {e}", routine.name);
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: friday-ctl routine save|run|rm <name> | list");
            ExitCode::from(2)
        }
    }
}

/// `friday-ctl local-ai detect | presets [--json] | model <budget_mb> |
/// local-first <model> <auto|local|cloud> [tokens/s]` (see
/// `friday_mobile::local_ai`): the same lines as the bash `friday local-ai`.
fn local_ai(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::local_ai as l;
    match args.first().map(String::as_str) {
        Some("detect") => print!("{}", l::detect().lines()),
        Some("presets") => {
            let rows = l::presets(&l::detect());
            if args.iter().any(|a| a == "--json") {
                let json: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "id": p.id, "label": p.label, "budget_mb": p.budget_mb,
                            "model": p.model, "download_mb": p.download_mb,
                            "vision": p.vision, "fast": p.fast,
                            "available": p.available, "note": p.note,
                        })
                    })
                    .collect();
                println!("{}", serde_json::Value::Array(json));
            } else {
                for p in rows {
                    println!("{}", p.line());
                }
            }
        }
        Some("model") => {
            let Some(mb) = args.get(1).and_then(|v| v.parse().ok()) else {
                eprintln!("usage: friday-ctl local-ai model <budget_mb>");
                return std::process::ExitCode::from(2);
            };
            println!("{}", l::model_for_budget(mb).map_or("", |m| m.id));
        }
        Some("local-first") => {
            let (Some(model), Some(order)) = (args.get(1), args.get(2)) else {
                eprintln!(
                    "usage: friday-ctl local-ai local-first <model> <auto|local|cloud> [tokens/s]"
                );
                return std::process::ExitCode::from(2);
            };
            // Like the bash script: no speed means unmeasured, one that isn't
            // a number counts as 0.
            let tps = args
                .get(3)
                .filter(|v| !v.is_empty())
                .map(|v| v.parse().unwrap_or(0.0));
            println!(
                "{}",
                l::local_first(model, l::Order::parse(order), tps) as u8
            );
        }
        _ => {
            eprintln!("usage: friday-ctl local-ai detect | presets [--json] | model <mb> | local-first <model> <order> [tps]");
            return std::process::ExitCode::from(2);
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl docs "<question>"`: excerpts from the documentation on this
/// computer (nothing when nothing fits). `--prepare` unpacks the Arch wiki
/// for searching; a search starts that in the background when it's due.
/// `friday-ctl workspace save <name> | restore <name> | list | rm <name>`:
/// keep the windows and the work in them, and put them back
/// (`friday_mobile::workspace`).
fn workspace(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::workspace as ws;
    use std::process::ExitCode;
    let dir = ws::dir();
    // --quiet (the session timer): no windows open is not an error, and
    // nothing is saved over the last session then.
    let quiet = args.iter().any(|a| a == "--quiet");
    let name = args
        .get(1..)
        .unwrap_or_default()
        .iter()
        .filter(|a| *a != "--quiet")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let proc_root = std::path::Path::new("/proc");
    let now = chrono::Utc::now().timestamp();
    match (args.first().map(String::as_str), name.is_empty()) {
        (Some("save"), false) => match ws::capture(&ws::SystemHypr, proc_root, &name, now)
            .and_then(|s| ws::save(&dir, &s).map(|_| s).map_err(|e| e.to_string()))
        {
            Ok(_) | Err(_) if quiet => ExitCode::SUCCESS,
            Ok(s) => {
                println!(
                    "Saved workspace \"{}\": {} window(s){}. Bring it back: friday workspace restore {}",
                    s.name,
                    s.windows.len(),
                    s.project.map(|p| format!(", project {p}")).unwrap_or_default(),
                    ws::key(&s.name)
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Couldn't save the workspace: {e}");
                ExitCode::FAILURE
            }
        },
        (Some("restore"), false) => {
            let Some(saved) = ws::find(&dir, &name) else {
                eprintln!("No saved workspace called \"{name}\" (friday workspace list).");
                return ExitCode::FAILURE;
            };
            match ws::restore(&ws::SystemHypr, proc_root, &saved) {
                Ok(report) => {
                    println!("{}", report.text(&saved));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("Couldn't restore the workspace: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        (Some("rm"), false) => match ws::find(&dir, &name) {
            Some(s)
                if std::fs::remove_file(dir.join(format!("{}.json", ws::key(&s.name)))).is_ok() =>
            {
                println!("Removed workspace \"{}\".", s.name);
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("No saved workspace called \"{name}\".");
                ExitCode::FAILURE
            }
        },
        (Some("project"), _) => {
            // The project of the window in front (`friday ask` tags its answers).
            if let Some(p) = project_in_front() {
                println!("{p}");
            }
            ExitCode::SUCCESS
        }
        (Some("list") | None, _) => {
            let all = ws::list(&dir);
            if all.is_empty() {
                println!("No saved workspaces. Save the windows you have open: friday workspace save <name>");
            }
            for s in all {
                let when = chrono::DateTime::from_timestamp(s.saved, 0)
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_default();
                println!(
                    "{}  {} window(s)  saved {when}{}",
                    s.name,
                    s.windows.len(),
                    s.project.map(|p| format!("  {p}")).unwrap_or_default()
                );
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: friday-ctl workspace save <name> | restore <name> | list | rm <name>"
            );
            ExitCode::from(2)
        }
    }
}

/// `friday-ctl mcp`: FRIDAY as an MCP server over stdio, one JSON-RPC
/// message per line (`friday_mobile::mcp_serve`). The allow list is read
/// again for every message, so `friday mcp allow` takes effect at once.
fn mcp_serve() -> std::process::ExitCode {
    use friday_mobile::{mcp_serve as mcp, thoughts as th, timeline};
    use std::io::{BufRead, Write};
    let state = friday_intelligence::default_state_dir();
    let run = |tool: &str, args: &serde_json::Value| -> Result<String, String> {
        let arg = |k: &str| {
            args.get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        match tool {
            "friday_thoughts" => {
                let text = th::render(&th::recent(&state, &arg("project"), 50, None));
                Ok(if text.is_empty() {
                    "No thoughts kept yet.".into()
                } else {
                    text
                })
            }
            "friday_timeline" => {
                let entries = timeline::collect(&state);
                let ask = match arg("when").as_str() {
                    "today" => "what did I do today",
                    "yesterday" => "what did I do yesterday",
                    "week" => "what did I do this week",
                    _ => "where was I",
                };
                Ok(match timeline::wants(ask, &entries, chrono::Local::now()) {
                    Some((from, to)) if !timeline::render(&entries, from, to).is_empty() => {
                        timeline::render(&entries, from, to)
                    }
                    _ => "Nothing in FRIDAY's logs for that yet.".into(),
                })
            }
            _ => {
                let question = arg("question");
                if question.is_empty() {
                    return Err("Ask a question (the question argument).".into());
                }
                let me = std::env::current_exe().map_err(|e| e.to_string())?;
                let out = std::process::Command::new(me)
                    .args(["goal", "--wait", &question])
                    .stderr(std::process::Stdio::null())
                    .output()
                    .map_err(|e| e.to_string())?;
                let text = String::from_utf8_lossy(&out.stdout);
                let last = serde_json::Deserializer::from_str(&text)
                    .into_iter::<serde_json::Value>()
                    .filter_map(Result::ok)
                    .filter(|v| v.get("outcome").is_some())
                    .last()
                    .unwrap_or_default();
                match (last["outcome"].as_str(), last["result"].as_str()) {
                    (Some("completed"), Some(answer)) => Ok(answer.to_string()),
                    _ => Err(last["error"]
                        .as_str()
                        .unwrap_or("FRIDAY didn't answer (is it running?)")
                        .to_string()),
                }
            }
        }
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let allow = std::fs::read_to_string(mcp::allow_file()).unwrap_or_default();
        if let Some(reply) = mcp::handle(&msg, &allow, &run) {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl timeline [today|yesterday|week]`: what FRIDAY's logs say
/// happened (the same text an ask about the past gets). Default: the last
/// day with anything on it.
fn timeline(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::timeline;
    let entries = timeline::collect(&friday_intelligence::default_state_dir());
    let ask = match args.first().map(String::as_str) {
        None => "where was I".to_string(),
        Some("today") => "what did I do today".to_string(),
        Some("yesterday") => "what did I do yesterday".to_string(),
        Some("week") => "what did I do this week".to_string(),
        Some(_) => {
            eprintln!("usage: friday-ctl timeline [today|yesterday|week]");
            return std::process::ExitCode::from(2);
        }
    };
    match timeline::wants(&ask, &entries, chrono::Local::now()) {
        Some((from, to)) if !timeline::render(&entries, from, to).is_empty() => {
            print!("{}", timeline::render(&entries, from, to));
        }
        _ => println!("Nothing in FRIDAY's logs for that yet."),
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl thought add [idea|decision|question|action] <text> | list [words]`:
/// keep a thought under the project in front (the terminal's git repo), and
/// read a thread back (`friday_mobile::thoughts`).
fn thought(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::thoughts::{self as th, Thought};
    use std::process::ExitCode;
    let state = friday_intelligence::default_state_dir();
    match args.first().map(String::as_str) {
        Some("add") => {
            let (kind, words) = match args.get(1).map(String::as_str) {
                Some(k) if th::KINDS.contains(&k) => (k, &args[2..]),
                _ => ("idea", &args[1..]),
            };
            let text = words.join(" ");
            if text.trim().is_empty() {
                eprintln!("usage: friday-ctl thought add [idea|decision|question|action] <text>");
                return ExitCode::from(2);
            }
            let project = project_in_front();
            let t = Thought {
                ts: chrono::Utc::now().timestamp(),
                project: project.clone(),
                kind: kind.to_string(),
                text,
            };
            match th::add(&state, &t) {
                Ok(()) => {
                    match project {
                        Some(p) => println!("Kept that {kind} under {}.", th::project_name(&p)),
                        None => println!("Kept that {kind}."),
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("Couldn't keep it: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("list") | None => {
            let goal = args.get(1..).unwrap_or_default().join(" ");
            let project =
                friday_mobile::project::resolve(&state, &friday_mobile::workspace::dir(), &goal)
                    .map(|p| th::project_name(&p.to_string_lossy()).to_string());
            let text = th::render(&th::recent(&state, &goal, 50, project.as_deref()));
            if text.is_empty() {
                println!("No thoughts kept yet. Say \"keep this idea: …\" in the ask box.");
            }
            print!("{text}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: friday-ctl thought add [idea|decision|question] <text> | list [project]"
            );
            ExitCode::from(2)
        }
    }
}

/// The project folder of the window in front: a terminal's git repository.
fn project_in_front() -> Option<String> {
    let out = std::process::Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()
        .ok()?;
    let w: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let pid = w["pid"].as_i64()?;
    let (dir, _) = friday_mobile::workspace::terminal_work(std::path::Path::new("/proc"), pid);
    friday_mobile::workspace::git_top(&dir?)
}

fn docs(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::local_docs;
    let s = local_docs::Sources::system();
    // `friday folders`: the chosen folders (one per line in
    // ~/.config/friday/search-folders), minus private ones (folder lines in
    // private-windows), made searchable.
    if args.first().map(String::as_str) == Some("--prepare-folders") {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
            })
            .unwrap_or_default()
            .join("friday");
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let paths = |file: &str, folders_only: bool| -> Vec<std::path::PathBuf> {
            std::fs::read_to_string(config.join(file))
                .unwrap_or_default()
                .lines()
                .map(|l| l.trim())
                .map(|l| l.strip_prefix("never ").map_or(l, str::trim))
                .filter(|l| !folders_only || l.starts_with('/') || l.starts_with("~/"))
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    l.strip_prefix("~/")
                        .map_or_else(|| l.into(), |r| home.join(r))
                })
                .collect()
        };
        let folders = paths("search-folders", false);
        return match local_docs::prepare_folders(
            &folders,
            &paths("private-windows", true),
            &s.folders,
        ) {
            Ok(n) => {
                println!(
                    "{n} files from {} folder(s) are ready to search.",
                    folders.len()
                );
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: couldn't read the folders: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if args.first().map(String::as_str) == Some("--prepare") {
        let mut code = std::process::ExitCode::SUCCESS;
        if s.wiki_gz.exists() {
            match local_docs::prepare(&s) {
                Ok(()) => println!("The Arch wiki is ready to search."),
                Err(e) => {
                    eprintln!("friday-ctl: couldn't unpack the Arch wiki: {e}");
                    code = std::process::ExitCode::FAILURE;
                }
            }
        }
        // tealdeer keeps its pages in a cache it fetches once (given two
        // minutes, so a network that never answers can't hold this forever).
        let tealdeer = std::process::Command::new("tldr")
            .arg("--version")
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("tealdeer"));
        if s.tldr.is_none() && tealdeer {
            let fetched = std::process::Command::new("timeout")
                .args(["120", "tldr", "--update"])
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|st| st.success());
            if fetched {
                println!("tldr pages fetched.");
            } else {
                eprintln!("friday-ctl: couldn't fetch tldr pages (tldr --update)");
            }
        }
        return code;
    }
    if s.needs_prepare() {
        // One try an hour: a marker younger than that means one is running,
        // or failed recently.
        let marker = s.cache.with_extension("preparing");
        let busy = std::fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(3600));
        if !busy {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&marker, "");
            if let Ok(exe) = std::env::current_exe() {
                let mut cmd = std::process::Command::new(exe);
                cmd.args(["docs", "--prepare"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                // Its own process group: `timeout` around this search kills
                // its whole group when time runs out.
                #[cfg(unix)]
                std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
                let _ = cmd.spawn();
            }
        }
    }
    // The question follows `--`, so one that reads "--prepare" is a question.
    let question = args.strip_prefix(&["--".to_string()]).unwrap_or(args);
    print!(
        "{}",
        local_docs::render(&local_docs::find(&question.join(" "), &s))
    );
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl show [--exact <name>] <what>`: where that control is in the
/// focused window, as JSON for `friday show`: `{"found": true, "name", "x",
/// "y", "width", "height"}` in screen pixels, or `{"found": false, "app",
/// "controls": [names]}` for the model to pick from, or `{"found": false,
/// "error"}`.
#[cfg(target_os = "linux")]
async fn show(args: &[String]) -> std::process::ExitCode {
    use friday_actions::{ActionAdapter, ActionRequest, UiAdapter};
    use friday_mobile::show_me::{self, Control};
    let say = |v: serde_json::Value| {
        println!("{v}");
        std::process::ExitCode::SUCCESS
    };
    let (exact, words) = match args {
        [flag, name, rest @ ..] if flag == "--exact" => (Some(name.as_str()), rest),
        _ => (None, args),
    };
    let question = words.join(" ");
    let win: serde_json::Value = std::process::Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();
    let (Some(pid), Some(wx), Some(wy)) = (
        win["pid"].as_u64(),
        win["at"][0].as_i64(),
        win["at"][1].as_i64(),
    ) else {
        return say(
            serde_json::json!({ "found": false, "error": "no window has focus (Hyprland)" }),
        );
    };
    let ui = UiAdapter::new();
    // One process can register more than one app (a Qt app loading a GTK
    // theme plugin does): the first of them with controls to show.
    let mut found_app = None;
    let mut controls: Vec<Control> = Vec::new();
    for app in ui.apps_for_pid(pid as u32).await {
        let listed = ui
            .execute(ActionRequest::new(
                "ui",
                "controls",
                serde_json::json!({ "app": app.id }),
            ))
            .await;
        controls = serde_json::from_value(listed.output).unwrap_or_default();
        if !controls.is_empty() {
            found_app = Some(app);
            break;
        }
    }
    let Some(app) = found_app else {
        return say(serde_json::json!({
            "found": false,
            "error": format!("FRIDAY can't see inside {}", win["class"].as_str().unwrap_or("this app")),
        }));
    };
    let found = match exact {
        Some(name) => show_me::named(name, &controls),
        None => show_me::pick(&question, &controls),
    };
    let Some(control) = found else {
        let names = show_me::names_for_model(&controls);
        return say(serde_json::json!({ "found": false, "app": app.name, "controls": names }));
    };
    let at = ui
        .execute(ActionRequest::new(
            "ui",
            "locate",
            serde_json::json!({ "app": app.id, "path": control.path }),
        ))
        .await;
    if !at.success {
        return say(serde_json::json!({ "found": false, "error": at.error }));
    }
    let o = &at.output;
    say(serde_json::json!({
        "found": true,
        "name": control.name,
        "x": wx + o["x"].as_i64().unwrap_or(0),
        "y": wy + o["y"].as_i64().unwrap_or(0),
        "width": o["width"],
        "height": o["height"],
    }))
}

#[cfg(not(target_os = "linux"))]
async fn show(_args: &[String]) -> std::process::ExitCode {
    println!(
        "{}",
        serde_json::json!({ "found": false, "error": "not on this system yet" })
    );
    std::process::ExitCode::SUCCESS
}

fn say(args: &[String]) -> std::process::ExitCode {
    if let Some(at) = args.iter().position(|a| a == "--setup") {
        let name = args.get(at + 1).map(String::as_str).unwrap_or("tinyvoca");
        let Some(kind) = friday_mobile::speech::kind_from_name(name) else {
            eprintln!(
                "friday-ctl: unknown voice \"{name}\" (tinyvoca, kokoro, supertonic or kitten)"
            );
            return std::process::ExitCode::from(2);
        };
        let size = if kind == friday_mobile::speech::Kind::TinyVoca {
            "about 50 MB"
        } else {
            "about 150 MB"
        };
        println!("Downloading FRIDAY's voice ({size}, once)...");
        return match friday_mobile::speech::setup(&friday_mobile::speech::voice_dir(), kind) {
            Ok(()) => {
                println!("Voice installed.");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    // The installed voice's names, one per line as "<number> <name>" (the number is what `voice N` takes).
    if args.iter().any(|a| a == "--voices") {
        let Some(voice) = friday_mobile::speech::detect(&friday_mobile::speech::voice_dir()) else {
            eprintln!("friday-ctl: FRIDAY's voice isn't installed");
            return std::process::ExitCode::from(1);
        };
        for (i, name) in friday_mobile::speech::voice_names(voice.kind)
            .iter()
            .enumerate()
        {
            println!("{i} {name}");
        }
        return std::process::ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--stop") {
        friday_mobile::speech::stop_running();
        return std::process::ExitCode::SUCCESS;
    }
    let text = if args == ["-"] {
        let mut t = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut t);
        t
    } else {
        args.join(" ")
    };
    match friday_mobile::speech::say(&text) {
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("friday-ctl: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("say") {
        return say(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("watch") {
        return watch(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("recall") {
        return recall(&raw[1..]).await;
    }
    if raw.first().map(String::as_str) == Some("routine") {
        return routine(&raw[1..]).await;
    }
    if raw.first().map(String::as_str) == Some("listen") {
        return match raw.get(1).map(String::as_str) {
            Some("captions") => std::process::ExitCode::from(friday_mobile::listen::run_captions(
                raw.iter().any(|a| a == "--mic"),
            )),
            Some("wake") => std::process::ExitCode::from(friday_mobile::listen::run_wake()),
            _ => {
                eprintln!("usage: friday-ctl listen captions [--mic] | wake");
                std::process::ExitCode::from(2)
            }
        };
    }
    if raw.first().map(String::as_str) == Some("local-ai") {
        return local_ai(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("docs") {
        return docs(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("workspace") {
        return workspace(&raw[1..]);
    }
    // The browser starts the host: through the Linux wrapper as
    // `browser-host`, or (Windows) directly, with the extension's origin
    // (Chromium) or the manifest's path (Firefox) as the first argument.
    let started_by_browser = raw.first().is_some_and(|a| {
        a == "browser-host"
            || a.starts_with("chrome-extension://")
            || a.ends_with("org.friday.browser.json")
    });
    if started_by_browser {
        return std::process::ExitCode::from(friday_mobile::browser_host::run_host());
    }
    if raw.first().map(String::as_str) == Some("browser")
        && raw.get(1).map(String::as_str) == Some("tab")
    {
        return match friday_mobile::browser_host::tab() {
            Ok(v) => {
                println!("{v}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if raw.first().map(String::as_str) == Some("mcp") {
        return mcp_serve();
    }
    if raw.first().map(String::as_str) == Some("thought") {
        return thought(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("timeline") {
        return timeline(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("show") {
        return show(&raw[1..]).await;
    }
    if raw.first().map(String::as_str) == Some("agent-hook") {
        return agent_hook(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("agent-hooks") {
        return agent_hooks(&raw[1..]);
    }
    // How each AI provider is doing, from the files next to state.json.
    if raw.first().map(String::as_str) == Some("providers") {
        let state_path = std::env::var("FRIDAY_STATE_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| ShellState::default_path());
        let dir = state_path.parent().unwrap_or(std::path::Path::new("."));
        print!(
            "{}",
            friday_mobile::provider_status::report(dir, chrono::Utc::now())
        );
        return std::process::ExitCode::SUCCESS;
    }

    // Resolve the daemon address + token, in precedence order: explicit CLI
    // flags, then env overrides, then the file-backed state.
    let state_path = if let Ok(p) = std::env::var("FRIDAY_STATE_FILE") {
        std::path::PathBuf::from(p)
    } else {
        ShellState::default_path()
    };
    // Address precedence: --addr, then FRIDAY_CTL_ADDR, then the address the
    // daemon published in state.json (it is not always the default port),
    // then the default.
    let mut addr_str = std::env::var("FRIDAY_CTL_ADDR")
        .ok()
        .or_else(|| read_listen(&state_path))
        .unwrap_or_else(|| "127.0.0.1:18432".to_string());
    let mut token = std::env::var("FRIDAY_CTL_TOKEN").ok().unwrap_or_default();
    let mut wait = false;
    let mut stream = false;
    // Optional goal fields: `--web`, `--thorough`, `--avoid <provider>/<model>`.
    let mut ask_opts = serde_json::Map::new();
    let mut question: Option<String> = None;
    let mut new_conversation = false;

    // Scan ALL tokens for flags (`--addr`/`--token`/`--wait` may appear before
    // or after the subcommand) so they never leak into the goal text. The first
    // non-flag token is the subcommand; the rest are goal words.
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    let mut confirm: Option<String> = None;
    let mut region: Option<String> = None;
    while i < raw.len() {
        let w = &raw[i];
        if w == "--addr" && i + 1 < raw.len() {
            addr_str = raw[i + 1].clone();
            i += 2;
        } else if w == "--token" && i + 1 < raw.len() {
            token = raw[i + 1].clone();
            i += 2;
        } else if w == "--ask" && i + 1 < raw.len() {
            question = Some(raw[i + 1].clone());
            i += 2;
        } else if w == "--region" && i + 1 < raw.len() {
            region = Some(raw[i + 1].clone());
            i += 2;
        } else if w == "--confirm" && i + 1 < raw.len() {
            confirm = Some(raw[i + 1].clone());
            i += 2;
        } else if w == "--wait" {
            wait = true;
            i += 1;
        } else if w == "--stream" {
            stream = true;
            i += 1;
        } else if w == "--web" {
            ask_opts.insert("web".into(), serde_json::json!(true));
            i += 1;
        } else if w == "--thorough" {
            ask_opts.insert("thorough".into(), serde_json::json!(true));
            i += 1;
        } else if w == "--new-conversation" {
            new_conversation = true;
            i += 1;
        } else if w == "--avoid" && i + 1 < raw.len() {
            // `provider/model`; the model id may itself hold a `/`.
            if let Some((provider, model)) = raw[i + 1].split_once('/') {
                ask_opts.insert(
                    "avoid".into(),
                    serde_json::json!({ "provider": provider, "model": model }),
                );
            }
            i += 2;
        } else {
            positional.push(w.clone());
            i += 1;
        }
    }

    let Some(sub) = positional.first() else {
        eprintln!("usage: friday-ctl <goal|cancel|system-info|context screen> [goal text] [--web] [--thorough [--avoid <provider>/<model>]] [--new-conversation] [--addr <host:port>] [--token <tok>]");
        return std::process::ExitCode::FAILURE;
    };
    let sub = sub.clone();
    let goal_words = positional.into_iter().skip(1).collect::<Vec<_>>();
    if sub == "goal" {
        ask_opts.insert(
            "conversation".into(),
            serde_json::json!(conversation_id(
                &state_path.with_file_name("conversation"),
                new_conversation,
                now_ms()
            )),
        );
    }

    if sub == "context" {
        // R5-2-5-3: consent + disclosure come first, before any daemon or
        // state-file access, so a decline never touches the daemon at all. The
        // provider sees nothing unless the user explicitly answers `y`.
        //
        // R5-2-5-3-1: `--yes` means the caller (the Omarchy panel) already
        // collected explicit consent via its Share button, so the terminal
        // prompt is skipped. `consent: true` is still sent and still enforced
        // by the frozen ContextGate — this only skips the TTY prompt, it never
        // weakens the gate.
        let channel = goal_words.first().map(String::as_str).unwrap_or("");
        if channel != "screen" {
            eprintln!("friday-ctl context <screen> — only 'screen' context is supported");
            return std::process::ExitCode::FAILURE;
        }
        let assume_consent = goal_words.iter().any(|w| w == "--yes");
        if !assume_consent && !confirm_context_consent() {
            eprintln!("Aborted — nothing was shared; the provider was never contacted.");
            return std::process::ExitCode::SUCCESS;
        }
    }

    let cmd_token = if token.is_empty() {
        match read_token(&state_path) {
            Some(t) => t,
            None => {
                let hint = if cfg!(windows) {
                    "start FRIDAY from the Start menu"
                } else {
                    "try: friday doctor"
                };
                eprintln!(
                    "no token found; FRIDAY isn't running (no {}): {hint}",
                    state_path.display()
                );
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        token
    };

    if sub == "context" {
        let region = match region.as_deref().map(parse_region) {
            None => None,
            Some(Some(r)) => Some(r),
            Some(None) => {
                eprintln!(
                    "friday-ctl: --region takes slurp's \"X,Y WxH\" (e.g. \"100,200 640x480\")"
                );
                return std::process::ExitCode::FAILURE;
            }
        };
        return run_context(&addr_str, &cmd_token, question.as_deref(), region, stream).await;
    }
    if sub == "cancel" {
        // Cancel the goal the daemon reports as in flight (state.json
        // `current`); the daemon matches it by run id.
        let Some(task_id) = read_current_goal_id(&state_path) else {
            eprintln!("no goal is running");
            return std::process::ExitCode::FAILURE;
        };
        return send_command(
            &addr_str,
            friday_mobile::method::TASK_CANCEL,
            serde_json::json!({ "token": cmd_token, "task_id": task_id }),
            Duration::from_secs(10),
            false,
        )
        .await;
    }
    let goal = goal_words.join(" ");
    let submitted_ms = now_ms();
    let accepted = run(
        &addr_str,
        &cmd_token,
        &sub,
        goal_words,
        confirm.as_deref(),
        &ask_opts,
    )
    .await;
    if sub == "goal" && wait && accepted == std::process::ExitCode::SUCCESS {
        return wait_for_goal(&state_path, &goal, submitted_ms, stream).await;
    }
    accepted
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `--wait`: follow the daemon's `state.json` until `goal` (submitted at
/// `since_ms`) reaches a terminal phase, then print its outcome. Exits
/// non-zero unless the goal completed. `FRIDAY_CTL_WAIT_SECS` bounds the wait
/// (default 180 s). With `stream` (`--stream`), the answer so far is printed
/// as it grows, one compact `{"partial": "..."}` line each time, before the
/// final outcome.
async fn wait_for_goal(
    state_path: &std::path::Path,
    goal: &str,
    since_ms: u64,
    stream: bool,
) -> std::process::ExitCode {
    use std::io::Write;
    let mut shown = String::new();
    let limit = wait_secs();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(limit);
    while tokio::time::Instant::now() < deadline {
        let state = std::fs::read_to_string(state_path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        if stream {
            if let Some(partial) = state.as_ref().and_then(|s| partial_answer(s, goal)) {
                if partial != shown {
                    let line = serde_json::json!({ "partial": partial });
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{line}");
                    let _ = out.flush();
                    shown = partial;
                }
            }
        }
        if let Some(done) = state.and_then(|s| finished_goal(&s, goal, since_ms)) {
            let mut summary = serde_json::json!({
                "outcome": done["outcome"],
                "result": done["result"],
                "error": done["error"],
            });
            if let Some(nc) = done.get("needs_confirmation").filter(|v| !v.is_null()) {
                summary["needs_confirmation"] = nc.clone();
            }
            if let Some(sb) = done.get("served_by").filter(|v| !v.is_null()) {
                summary["served_by"] = sb.clone();
            }
            if let Some(u) = done.get("used").filter(|v| !v.is_null()) {
                summary["used"] = u.clone();
            }
            if let Some(m) = done.get("masked").filter(|v| !v.is_null()) {
                summary["masked"] = m.clone();
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
            return if done["outcome"] == "completed" {
                std::process::ExitCode::SUCCESS
            } else if done
                .get("needs_confirmation")
                .is_some_and(|nc| !nc.is_null())
            {
                std::process::ExitCode::from(3u8)
            } else {
                std::process::ExitCode::FAILURE
            };
        }
        let poll = if stream { 100 } else { 300 };
        tokio::time::sleep(Duration::from_millis(poll)).await;
    }
    eprintln!("timed out after {limit}s waiting for the goal to finish");
    std::process::ExitCode::FAILURE
}

/// `friday-ctl agent-hook claude`: Claude Code's PermissionRequest hook.
/// Reads the request on stdin; prints FRIDAY's approval, or nothing to leave
/// the prompt to the person. Never fails the hook: any problem means "ask".
fn agent_hook(args: &[String]) -> std::process::ExitCode {
    use std::io::Read;
    let agent = args.first().map(String::as_str).unwrap_or("");
    if agent != "claude" && agent != "agy" {
        eprintln!(
            "usage: friday-ctl agent-hook claude|agy   (the agent runs this; it reads stdin)"
        );
        return std::process::ExitCode::from(2u8);
    }
    let mut input = String::new();
    let off = std::io::stdin().read_to_string(&mut input).is_err()
        || !friday_mobile::agent_hooks::enabled();
    let request = serde_json::from_str::<serde_json::Value>(&input).ok();
    let log_dir = friday_mobile::agent_runs::runs_dir();
    match (agent, request) {
        ("claude", Some(r)) if !off => {
            if let Some(answer) = friday_mobile::agent_hooks::claude_permission(&r, &log_dir) {
                println!("{answer}");
            }
        }
        ("agy", Some(r)) if !off => {
            println!("{}", friday_mobile::agent_hooks::agy_pre_tool(&r, &log_dir));
        }
        // agy wants an answer every time: when FRIDAY can't decide, agy asks
        // the person as it would without FRIDAY.
        ("agy", _) => println!("{}", serde_json::json!({"decision": "ask"})),
        _ => {}
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl agent-hooks on|off|status`: FRIDAY in the person's own agent
/// sessions: its hooks in Claude Code's and agy's settings, and the opencode
/// watcher's switch.
fn agent_hooks(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::agent_hooks::*;
    let (Some(claude), Some(agy)) = (claude_settings(), agy_hooks()) else {
        eprintln!("no home folder");
        return std::process::ExitCode::FAILURE;
    };
    let off_marker = sessions_off_marker();
    let result: Result<(), String> = match args.first().map(String::as_str).unwrap_or("status") {
        "status" => {
            let line = |name: &str, on: bool| {
                if on {
                    println!("{name}: FRIDAY answers routine approvals in your own sessions; risky ones stay with you.");
                } else {
                    println!("{name}: FRIDAY isn't answering approvals in your own sessions.");
                }
            };
            line("Claude Code", installed_claude(&claude));
            line("agy", installed_agy(&agy));
            line("opencode", !off_marker.exists());
            println!("Turn all on or off: friday agents hooks on|off");
            Ok(())
        }
        "on" => std::env::current_exe().map_err(|e| e.to_string()).and_then(|ctl| {
            install_claude(&claude, &ctl)?;
            install_agy(&agy, &ctl)?;
            let _ = std::fs::remove_file(&off_marker);
            println!("FRIDAY now answers routine approvals in your own Claude Code, agy and opencode sessions (new sessions pick it up).");
            Ok(())
        }),
        "off" => uninstall_claude(&claude).and_then(|_| uninstall_agy(&agy)).and_then(|_| {
            if let Some(dir) = off_marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&off_marker, "").map_err(|e| e.to_string())?;
            println!("FRIDAY no longer answers approvals in your own agent sessions.");
            Ok(())
        }),
        _ => Err("usage: friday-ctl agent-hooks on|off|status".into()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The state document's `last_goal` when it is `goal`, finished, and
/// completed no earlier than `since_ms` (so an older run of the same goal
/// text is not mistaken for this one).
fn finished_goal(
    state: &serde_json::Value,
    goal: &str,
    since_ms: u64,
) -> Option<serde_json::Value> {
    let last = state.get("last_goal")?;
    let (masked_goal, _) = friday_goal_execution::mask_secrets(goal);
    let finished = (last["goal"] == goal || last["goal"] == masked_goal)
        && last["active"] != true
        && last["completed_ms"]
            .as_u64()
            .is_some_and(|ms| ms >= since_ms);
    finished.then(|| last.clone())
}

/// The answer so far of `goal` while it is in flight (state.json's
/// `current.partial`).
fn partial_answer(state: &serde_json::Value, goal: &str) -> Option<String> {
    let cur = state.get("current")?;
    let (masked_goal, _) = friday_goal_execution::mask_secrets(goal);
    if (cur["goal"] != goal && cur["goal"] != masked_goal) || cur["active"] != true {
        return None;
    }
    cur["partial"]
        .as_str()
        .filter(|p| !p.trim().is_empty())
        .map(str::to_string)
}

/// The id of the goal the daemon reports as in flight, if any.
fn read_current_goal_id(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    st.current.map(|g| g.id).filter(|id| !id.is_empty())
}

/// The ingress address the daemon published in state.json, if any.
fn read_listen(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    (!st.listen.is_empty()).then_some(st.listen)
}

/// How long a conversation stays open between asks.
const CONVERSATION_IDLE_MS: u64 = 15 * 60 * 1000;

/// The conversation this ask belongs to: the one in `path` ("<id> <last ask
/// ms>") while it was used in the last 15 minutes, else a new one (always a
/// new one with `--new-conversation`). Shared by every client that asks
/// through friday-ctl: the CLI, FRIDAY Shell and the Windows tray.
fn conversation_id(path: &std::path::Path, fresh: bool, now: u64) -> String {
    let kept = std::fs::read_to_string(path).ok().and_then(|text| {
        let (id, at) = text.trim().split_once(' ')?;
        let at: u64 = at.parse().ok()?;
        (!fresh && now.saturating_sub(at) < CONVERSATION_IDLE_MS && !id.is_empty())
            .then(|| id.to_string())
    });
    let id = kept.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let _ = std::fs::write(path, format!("{id} {now}"));
    id
}

fn read_token(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let st: ShellState = serde_json::from_str(&text).ok()?;
    if st.token.is_empty() {
        return None;
    }
    Some(st.token)
}

async fn run(
    addr: &str,
    token: &str,
    sub: &str,
    args: Vec<String>,
    confirm: Option<&str>,
    ask_opts: &serde_json::Map<String, serde_json::Value>,
) -> std::process::ExitCode {
    // Build the command. Only `goal`, `system-info`, and `context` are wired
    // for now — the "one controlled command" surface.
    let (method, params) = match sub {
        "goal" => {
            let goal = args.join(" ");
            if goal.trim().is_empty() {
                eprintln!("friday-ctl goal <text>: goal text required");
                return std::process::ExitCode::FAILURE;
            }
            let mut p = serde_json::json!({ "goal": goal, "token": token });
            if let Some(c) = confirm {
                p["confirm"] = serde_json::json!(c);
            }
            for (k, v) in ask_opts {
                p[k] = v.clone();
            }
            (friday_mobile::method::PLANNER_GOAL.to_string(), p)
        }
        "system-info" => (
            friday_mobile::method::SYSTEM_INFO.to_string(),
            serde_json::json!({ "token": token }),
        ),
        other => {
            eprintln!("unknown subcommand: {other}");
            return std::process::ExitCode::FAILURE;
        }
    };
    send_command(addr, &method, params, Duration::from_secs(10), false).await
}

/// R5-2-5-3: send the Screen `context.read`. `main` has already presented
/// disclosure and collected explicit consent; this only runs after the user
/// answered `y`, so it sends with a per-use `consent: true`.
/// The model answers the screenshot before the result comes back, so this
/// waits as long as `goal --wait` does (`FRIDAY_CTL_WAIT_SECS`, default 180 s),
/// not a plain command's 10 s; `stream` prints the answer as it arrives.
async fn run_context(
    addr: &str,
    token: &str,
    question: Option<&str>,
    region: Option<serde_json::Value>,
    stream: bool,
) -> std::process::ExitCode {
    let mut params = serde_json::json!({ "token": token, "consent": true });
    if let Some(q) = question {
        params["question"] = serde_json::Value::String(q.to_string());
    }
    if let Some(r) = region {
        params["region"] = r;
    }
    let wait = Duration::from_secs(wait_secs());
    send_command(
        addr,
        friday_mobile::method::CONTEXT_READ,
        params,
        wait,
        stream,
    )
    .await
}

/// `--region`: slurp's "X,Y WxH" as the daemon's `{x, y, w, h}`.
fn parse_region(s: &str) -> Option<serde_json::Value> {
    let (pos, size) = s.trim().split_once(' ')?;
    let (x, y) = pos.split_once(',')?;
    let (w, h) = size.trim().split_once('x')?;
    let n = |v: &str| v.trim().parse::<i64>().ok();
    let (x, y, w, h) = (n(x)?, n(y)?, n(w)?, n(h)?);
    (w > 0 && h > 0).then(|| serde_json::json!({ "x": x, "y": y, "w": w, "h": h }))
}

/// How long to wait for an answer: `FRIDAY_CTL_WAIT_SECS`, default 180 s.
fn wait_secs() -> u64 {
    std::env::var("FRIDAY_CTL_WAIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180)
}

/// The partial answer a `context.progress` event carries for command `id`.
fn context_progress(msg: &ServerMessage, id: &str) -> Option<String> {
    match msg {
        ServerMessage::Event { topic, payload }
            if topic == "context.progress" && payload["id"] == id =>
        {
            payload["partial"].as_str().map(str::to_string)
        }
        _ => None,
    }
}

/// Connect to the daemon, send one command, and await the matching
/// `CommandResult` for up to `wait`. Other messages are skipped, except that
/// with `stream` this command's `context.progress` answers are printed as
/// `{"partial": ...}` lines as they arrive.
async fn send_command(
    addr: &str,
    method: &str,
    params: serde_json::Value,
    wait: Duration,
    stream: bool,
) -> std::process::ExitCode {
    use std::io::Write;
    let ws_url = format!("ws://{addr}");
    let (mut ws, _) = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot reach daemon at {ws_url}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let want_id = id.clone();

    // In open mode the server accepts a command directly; send ours and read
    // the matching CommandResult. Any welcome/events are consumed first.
    let client_msg = ClientMessage::Command {
        id: id.clone(),
        method: method.to_string(),
        params,
    };
    if let Err(e) = ws
        .send(Message::Text(serde_json::to_string(&client_msg).unwrap()))
        .await
    {
        eprintln!("couldn't send the command to the daemon: {e}");
        return std::process::ExitCode::FAILURE;
    }

    // Loop until we see our CommandResult (skipping welcome/events).
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if tokio::time::Instant::now() > deadline {
            eprintln!("timed out waiting for command result");
            return std::process::ExitCode::FAILURE;
        }
        let msg = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                eprintln!("ws error: {e}");
                return std::process::ExitCode::FAILURE;
            }
            Ok(None) => {
                eprintln!(
                    "the daemon closed the connection before it answered (is it restarting?)"
                );
                return std::process::ExitCode::FAILURE;
            }
            // Nothing for a while: keep waiting until the deadline.
            Err(_) => continue,
        };
        if let Message::Text(t) = msg {
            let decoded = decode_server(&t);
            if let (true, Ok(m)) = (stream, &decoded) {
                if let Some(partial) = context_progress(m, &want_id) {
                    let line = serde_json::json!({ "partial": partial });
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{line}");
                    let _ = out.flush();
                    continue;
                }
            }
            if let Ok(ServerMessage::CommandResult { id, ok, result }) = decoded {
                if id == want_id {
                    let json = serde_json::to_string_pretty(&result).unwrap_or_default();
                    println!("{json}");
                    return if ok {
                        std::process::ExitCode::SUCCESS
                    } else {
                        std::process::ExitCode::FAILURE
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn asks_share_a_conversation_until_it_goes_quiet() {
        let path = std::env::temp_dir().join(format!("friday-ctl-conv-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let first = conversation_id(&path, false, 1_000);
        assert_eq!(
            conversation_id(&path, false, 60_000),
            first,
            "a minute later: same one"
        );
        let later = 60_000 + CONVERSATION_IDLE_MS + 1;
        let quiet = conversation_id(&path, false, later);
        assert_ne!(quiet, first, "after 15 quiet minutes: a new one");
        assert_ne!(
            conversation_id(&path, true, later + 1),
            quiet,
            "--new-conversation"
        );
        let _ = std::fs::remove_file(path);
    }

    use super::*;
    use serde_json::json;

    /// A daemon that hangs up without answering fails the command at once: it used to
    /// spin until the deadline and then say it had timed out.
    #[tokio::test]
    async fn a_daemon_that_hangs_up_fails_the_command_at_once() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            let _ = ws.close(None).await;
        });
        let started = std::time::Instant::now();
        let code = send_command(&addr, "x", json!({}), Duration::from_secs(60), false).await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", std::process::ExitCode::FAILURE)
        );
    }

    fn state(goal: &str, active: bool, completed_ms: Option<u64>) -> serde_json::Value {
        json!({ "last_goal": {
            "goal": goal, "active": active, "completed_ms": completed_ms,
            "outcome": "completed", "result": "done", "error": null
        }})
    }

    #[test]
    fn context_progress_is_only_this_commands() {
        let ev = |id: &str| ServerMessage::Event {
            topic: "context.progress".to_string(),
            payload: json!({"id": id, "partial": "The screen sh"}),
        };
        assert_eq!(
            context_progress(&ev("c1"), "c1").as_deref(),
            Some("The screen sh")
        );
        assert_eq!(context_progress(&ev("other"), "c1"), None);
        let other_topic = ServerMessage::Event {
            topic: "goal.progress".to_string(),
            payload: json!({"id": "c1", "partial": "x"}),
        };
        assert_eq!(context_progress(&other_topic, "c1"), None);
    }

    #[test]
    fn partial_answer_is_only_the_goal_in_flight() {
        let st = serde_json::json!({
            "current": {"goal": "q", "active": true, "partial": "The ans"}
        });
        assert_eq!(partial_answer(&st, "q").as_deref(), Some("The ans"));
        assert_eq!(partial_answer(&st, "other"), None);
        let done = serde_json::json!({
            "current": {"goal": "q", "active": false, "partial": "The ans"}
        });
        assert_eq!(partial_answer(&done, "q"), None);
        let none = serde_json::json!({ "current": {"goal": "q", "active": true} });
        assert_eq!(partial_answer(&none, "q"), None);
    }

    #[test]
    fn wait_matches_only_this_goal_once_it_has_finished() {
        assert!(finished_goal(
            &state("open chromium", false, Some(200)),
            "open chromium",
            100
        )
        .is_some());
        assert!(
            finished_goal(
                &state("open chromium", true, Some(200)),
                "open chromium",
                100
            )
            .is_none(),
            "still running"
        );
        assert!(
            finished_goal(&state("open chromium", false, None), "open chromium", 100).is_none(),
            "not completed"
        );
        assert!(
            finished_goal(
                &state("open chromium", false, Some(50)),
                "open chromium",
                100
            )
            .is_none(),
            "an earlier run"
        );
        assert!(
            finished_goal(&state("open files", false, Some(200)), "open chromium", 100).is_none(),
            "another goal"
        );
        assert!(finished_goal(&json!({ "last_goal": null }), "open chromium", 100).is_none());
    }

    #[test]
    fn region_from_slurp() {
        assert_eq!(
            parse_region("100,-20 640x480"),
            Some(serde_json::json!({ "x": 100, "y": -20, "w": 640, "h": 480 }))
        );
        assert_eq!(parse_region("100,200 0x480"), None);
        assert_eq!(parse_region("100 200 640x480"), None);
        assert_eq!(parse_region(""), None);
    }
}
