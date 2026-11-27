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
//!   friday-ctl goal --json "..."          (for scripts: waits, prints one
//!                                            JSON object, exit codes in
//!                                            specs/ask-json.md)
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
//!   friday-ctl context screen --yes --picture --ask "what does this error mean?"
//!                                      (answer with a screenshot, not the window's text)
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
    let pidfile = friday_mobile::runtime_dir().join("friday-watch.pid");
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
        eprintln!("usage: friday-ctl watch kitty <socket> <window id> <command> | region <X,Y WxH> | until <command> | gone <title words> | stop");
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
        Some("until") if args.len() >= 2 => {
            let cmd = args[1..].join(" ");
            if let Some(why) = friday_actions::blocklist::never_run(&cmd) {
                eprintln!("FRIDAY never runs that: {why}.");
                return ExitCode::from(2);
            }
        }
        Some("gone") if args.len() >= 2 => {
            let clients = Command::new("hyprctl")
                .args(["clients", "-j"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            if w::window_open(&clients, &args[1..].join(" ")) != Some(true) {
                println!("No window has \"{}\" in its title.", args[1..].join(" "));
                return ExitCode::FAILURE;
            }
        }
        _ => return usage(),
    }
    stop_running();
    let _ = std::fs::write(&pidfile, std::process::id().to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(w::MAX_SECS);
    let mut region = Region::default();
    let me = std::process::id().to_string();
    let mut blind = 0;
    let mut looks = 0;
    while std::time::Instant::now() < deadline {
        // Replaced by a newer watch (or stopped): this one ends quietly.
        if std::fs::read_to_string(&pidfile).ok().as_deref() != Some(me.as_str()) {
            return ExitCode::SUCCESS;
        }
        if args[0] == "until" || args[0] == "gone" {
            let what = args[1..].join(" ");
            let done = if args[0] == "until" {
                Command::new("timeout")
                    .args(["60", "sh", "-c", &what])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            } else {
                let clients = Command::new("hyprctl")
                    .args(["clients", "-j"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                w::window_open(&clients, &what) == Some(false)
            };
            if done {
                let body = if args[0] == "until" {
                    format!("This works now: {what}")
                } else {
                    format!("The \"{what}\" window closed.")
                };
                notify("FRIDAY", &body);
                break;
            }
            // Self-paced: less often the longer it takes.
            let wait = Duration::from_secs(w::paced(looks));
            looks += 1;
            let until = std::time::Instant::now() + wait;
            while std::time::Instant::now() < until.min(deadline) {
                if std::fs::read_to_string(&pidfile).ok().as_deref() != Some(me.as_str()) {
                    return ExitCode::SUCCESS;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            continue;
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

/// `friday-ctl window-text <class> <title>`: the words of that window (the
/// one the caller checked, even if another took focus since), read from its
/// accessibility tree on this computer (nothing is sent anywhere), for an
/// ask that must stay local (a private window and the local model). Fails
/// when the window has too few words to read.
async fn window_text(args: &[String]) -> std::process::ExitCode {
    use friday_mobile::context_read::ScreenText;
    let [class, title] = args else {
        eprintln!("usage: friday-ctl window-text <class> <title>");
        return std::process::ExitCode::from(2);
    };
    if class.is_empty() {
        eprintln!("friday-ctl: no window given");
        return std::process::ExitCode::from(2);
    }
    let Some(reader) = friday_mobile::agent_desktop::detect().await else {
        eprintln!("friday-ctl: reading a window needs agent-desktop");
        return std::process::ExitCode::FAILURE;
    };
    match reader.window_text(class, title).await {
        Some(text) => {
            println!("{text}");
            std::process::ExitCode::SUCCESS
        }
        None => {
            eprintln!("friday-ctl: that window has too little text to read");
            std::process::ExitCode::FAILURE
        }
    }
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
            loop {
                // Nothing is read while the screen is locked: what changes
                // behind the lock (a chat, a notification) wasn't looked at.
                let locked = std::process::Command::new("pidof")
                    .args(["hyprlock", "swaylock", "gtklock", "waylock"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if locked {
                    tokio::time::sleep(Duration::from_secs(recall::EVERY_SECS)).await;
                    continue;
                }
                let win: serde_json::Value = std::process::Command::new("hyprctl")
                    .args(["activewindow", "-j"])
                    .output()
                    .ok()
                    .and_then(|o| serde_json::from_slice(&o.stdout).ok())
                    .unwrap_or_default();
                let app = win["class"].as_str().unwrap_or_default();
                let title = win["title"].as_str().unwrap_or_default();
                let own_list = recall::own_list();
                // The reader takes only the window in front; still, while
                // any window of the app is private, none of it is kept.
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
                    if let Some(text) = reader.window_text(app, title).await {
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
    let recording = friday_mobile::runtime_dir().join("friday-recording.txt");
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
        // After an ask: when FRIDAY has now done the same desktop steps
        // three times, offer (once) to keep them as a routine.
        Some("suggest") => {
            let Some(audit) = friday_mobile::desktop_tool::audit_dir() else {
                return ExitCode::SUCCESS;
            };
            let log = std::fs::read_to_string(audit.join("actions.jsonl")).unwrap_or_default();
            let Some((rec, times)) = r::repeated(&log, &r::list(&dir)) else {
                return ExitCode::SUCCESS;
            };
            use sha2::Digest;
            let key: String = sha2::Sha256::digest(
                serde_json::to_string(&rec.steps)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
            let offered = audit.join("routine-offered");
            let seen = std::fs::read_to_string(&offered).unwrap_or_default();
            if seen.lines().any(|l| l == key) {
                return ExitCode::SUCCESS;
            }
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&offered)
                .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{key}\n").as_bytes()));
            println!(
                "FRIDAY has done these {} steps {times} times now. Keep them as a routine: friday routine save <name>",
                rec.steps.len()
            );
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
            eprintln!("usage: friday-ctl local-ai detect | presets [--json] | engines [--json] | pull <model> | install-llamacpp <model> [--gpu|--cpu] | model <mb> | local-first <model> <order> [tps]");
            return std::process::ExitCode::from(2);
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl local-ai engines [--json]`: Ollama and llama.cpp on this
/// computer, running or not, with the models each already has.
async fn local_ai_engines(json: bool) -> std::process::ExitCode {
    use friday_mobile::local_ai as l;
    let engines = l::engines().await;
    if json {
        let v: Vec<serde_json::Value> = engines
            .iter()
            .map(|e| {
                serde_json::json!({
                    "engine": e.name, "installed": e.installed, "running": e.running,
                    "base_url": format!("{}/v1", e.base_url),
                    "models": e.models.iter().map(|m| serde_json::json!({
                        "name": m.name, "size_mb": m.size_mb, "vision": m.vision,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(v));
    } else {
        for e in &engines {
            println!("{}", l::engine_line(e));
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `friday-ctl models rank-screen [--json]`: ranks models on screen questions.
async fn models(args: &[String]) -> std::process::ExitCode {
    if args.first().map(String::as_str) != Some("rank-screen") {
        eprintln!("usage: friday-ctl models rank-screen [--json]");
        return std::process::ExitCode::from(2);
    }
    let json = args.iter().any(|a| a == "--json");
    // The keys the daemon reads (friday.env, which systemd hands it): this
    // runs from a terminal, so read them here too. Plain KEY=value lines
    // only, never run as a script; one already set in the environment wins.
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")));
    if let Some(dir) = config {
        let text = std::fs::read_to_string(dir.join("friday/friday.env")).unwrap_or_default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            let k = k.trim();
            if !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && std::env::var_os(k).is_none()
            {
                std::env::set_var(k, v.trim().trim_matches('"'));
            }
        }
        friday_mobile::secrets::resolve_env();
    }

    // A folder of its own for provider health and timings: a benchmark's
    // slow or refused answers must never change how FRIDAY routes your asks.
    let fabric_dir = std::env::temp_dir().join(format!("friday-rank-{}", std::process::id()));
    // Removed on every way out of here.
    struct RemoveDir(std::path::PathBuf);
    impl Drop for RemoveDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = RemoveDir(fabric_dir.clone());
    let mut fabric = friday_intelligence::InferenceFabric::default_from_env_in(&fabric_dir);
    // A vision model on a free tier can be slow: the screen ask's own limit.
    fabric.request_timeout = Some(std::time::Duration::from_secs(45));
    fabric.screen_request_timeout = Some(std::time::Duration::from_secs(45));

    let needs_vision = friday_intelligence::RequestRequirements {
        vision: friday_intelligence::Requirement::Required,
        ..Default::default()
    };
    let mut vision: Vec<String> = fabric
        .models
        .iter()
        .filter(|m| {
            m.capabilities
                .contains(&friday_intelligence::ModelCapability::Vision)
        })
        .map(|m| m.provider.clone())
        .collect();
    vision.sort();
    vision.dedup();

    let candidates =
        match friday_mobile::context_read::screen_route(&fabric.user_providers(), &vision) {
            friday_mobile::context_read::ScreenRoute::Defaults => {
                fabric.select_preferring(&needs_vision, None, None)
            }
            friday_mobile::context_read::ScreenRoute::Among(providers) => {
                fabric.select_preferring(&needs_vision, None, Some(&providers))
            }
            friday_mobile::context_read::ScreenRoute::Refuse(why) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "outcome": "refused",
                            "error": why,
                            "models": []
                        })
                    );
                } else {
                    eprintln!("friday-ctl: {why}");
                }
                return std::process::ExitCode::from(1);
            }
        };

    let mut model_names = Vec::new();
    for (m, _) in &candidates {
        let name = format!("{}/{}", m.provider, m.id);
        if !model_names.contains(&name) {
            model_names.push(name);
        }
    }
    model_names.truncate(6);

    if model_names.is_empty() {
        if json {
            println!("{}", serde_json::json!([]));
        } else {
            println!("No vision-capable models available to rank.");
        }
        return std::process::ExitCode::SUCCESS;
    }

    if !json {
        let total_reqs = model_names.len() * 8;
        println!(
            "Requests go to your providers: {} requests ({} models x 8).",
            total_reqs,
            model_names.len()
        );
    }

    struct FabricAsk {
        fabric: std::sync::Arc<friday_intelligence::InferenceFabric>,
    }

    #[async_trait::async_trait]
    impl friday_mobile::screen_rank::Ask for FabricAsk {
        async fn ask(&self, model: &str, prompt: &str) -> Result<(String, u64), String> {
            let (provider, id) = model.split_once('/').unwrap_or(("", model));
            let start = std::time::Instant::now();
            let req = friday_intelligence::RequestRequirements {
                vision: friday_intelligence::Requirement::Required,
                ..Default::default()
            };
            tracing::debug!(
                "screen_rank asking model {} with prompt len {}",
                model,
                prompt.len()
            );
            let res = self
                .fabric
                .complete_preferring(
                    &req,
                    Some(model),
                    Some(&[provider.to_string()]),
                    prompt,
                    None,
                )
                .await;
            let elapsed_ms = start.elapsed().as_millis() as u64;

            match res {
                Ok(text) => {
                    tracing::debug!(
                        "screen_rank received response from model {} len {}",
                        model,
                        text.len()
                    );
                    let served = {
                        let d = self.fabric.last_decision.lock().unwrap();
                        d.as_ref().and_then(|dec| dec.served.clone())
                    };
                    if let Some((p, m)) = served {
                        if p == provider && m == id {
                            return Ok((text, elapsed_ms));
                        }
                    }
                    Err(format!("model {} did not serve response", model))
                }
                Err(e) => Err(e),
            }
        }
    }

    let fabric = std::sync::Arc::new(fabric);
    let rows = friday_mobile::screen_rank::rank(&model_names, FabricAsk { fabric }).await;

    if json {
        let out: Vec<serde_json::Value> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                serde_json::json!({
                    "rank": i + 1,
                    "model": r.model,
                    "correct": r.correct,
                    "total": 8,
                    "median_ms": r.median_ms,
                    "median_s": (r.median_ms as f64) / 1000.0,
                    "failed": r.failed,
                    "rate_limited": r.rate_limited,
                    "unavailable": r.unavailable(),
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(out));
        return std::process::ExitCode::SUCCESS;
    }

    println!();
    println!(
        "{:<5} {:<35} {:<12} {:<10}",
        "Rank", "Model", "Correct", "Median"
    );
    for (i, r) in rows.iter().enumerate() {
        let rank_str = format!("{}", i + 1);
        let correct_str = format!("{}/8", r.correct);
        let median_str = if r.rate_limited && r.correct == 0 {
            "rate limited".to_string()
        } else if r.unavailable() {
            "unavailable".to_string()
        } else {
            format!("{:.2}s", (r.median_ms as f64) / 1000.0)
        };
        println!(
            "{:<5} {:<35} {:<12} {:<10}",
            rank_str, r.model, correct_str, median_str
        );
    }

    if rows.iter().any(|r| r.rate_limited) {
        println!("\n\"rate limited\": that provider said too many requests (free tiers cap how often you ask); try again later.");
    }
    // Only a model that answered something right is worth recommending.
    match rows.iter().find(|r| !r.unavailable() && r.correct > 0) {
        Some(best) => println!(
            "\nUse the best for screen asks: friday model screen {}",
            best.model
        ),
        None => println!("\nNo model answered these questions right now; nothing to recommend."),
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
                    // Its own conversation, and every action that can ask
                    // first does: the app's words aren't the person's.
                    .args([
                        "goal",
                        "--wait",
                        "--new-conversation",
                        &mcp::from_another_app(&question),
                    ])
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

/// `friday-ctl day [today|yesterday|YYYY-MM-DD]`: a short summary of a day
/// by project, built here from FRIDAY's logs and kept in `days/` (so it
/// outlasts the logs); `friday-ctl day forget <YYYY-MM-DD|all>` deletes kept
/// summaries.
fn day(args: &[String], state: &std::path::Path) -> std::process::ExitCode {
    use friday_mobile::timeline;
    use std::process::ExitCode;
    let days = state.join("days");
    let date = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let today = chrono::Local::now().date_naive();
    if args.first().map(String::as_str) == Some("forget") {
        return match args.get(1).map(String::as_str) {
            Some("all") => {
                let n = std::fs::read_dir(&days).map(|d| d.count()).unwrap_or(0);
                let _ = std::fs::remove_dir_all(&days);
                println!("Deleted {n} kept day summaries.");
                ExitCode::SUCCESS
            }
            Some(s) if date(s).is_some() => {
                match std::fs::remove_file(days.join(format!("{s}.md"))) {
                    Ok(()) => println!("Deleted the summary of {s}."),
                    Err(_) => println!("There's no kept summary of {s}."),
                }
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("usage: friday-ctl day forget <YYYY-MM-DD|all>");
                ExitCode::from(2)
            }
        };
    }
    let Some(on) = (match args.first().map(String::as_str) {
        None | Some("today") => Some(today),
        Some("yesterday") => today.pred_opt(),
        Some(s) => date(s),
    }) else {
        eprintln!(
            "usage: friday-ctl day [today|yesterday|YYYY-MM-DD] | day forget <YYYY-MM-DD|all>"
        );
        return ExitCode::from(2);
    };
    let kept = days.join(format!("{}.md", on.format("%Y-%m-%d")));
    let text = timeline::day_summary(&timeline::collect(state), on);
    if text.is_empty() {
        match std::fs::read_to_string(&kept) {
            Ok(t) => print!("{t}"),
            Err(_) => println!("Nothing in FRIDAY's logs for {}.", on.format("%Y-%m-%d")),
        }
        return ExitCode::SUCCESS;
    }
    let (masked, _) = friday_goal_execution::mask_secrets(&text);
    print!("{masked}");
    if std::fs::create_dir_all(&days).is_ok() {
        let _ = std::fs::write(&kept, &masked);
    }
    ExitCode::SUCCESS
}

/// `friday-ctl answers search <words…> [--since today|yesterday|week|month|<N>d]`:
/// past answers holding every word, newest first, with their dates.
/// `friday-ctl answers told <ask>`: the same for "what did you tell me
/// about X last week" (exit 1 when the ask isn't that). Read here only.
fn answers(args: &[String], state: &std::path::Path) -> std::process::ExitCode {
    use friday_mobile::timeline;
    use std::process::ExitCode;
    let text = ["answers.1.md", "answers.md"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(state.join(f)).ok())
        .collect::<String>();
    let now = chrono::Local::now();
    let (words, since, topic) = match args.first().map(String::as_str) {
        Some("told") => match timeline::told_about(&args[1..].join(" "), now) {
            Some((topic, since)) => (timeline::search_words(&topic), since, topic),
            None => return ExitCode::FAILURE,
        },
        Some("search") => {
            let mut words = Vec::new();
            let mut since = 0;
            let mut rest = args[1..].iter();
            while let Some(a) = rest.next() {
                if a != "--since" {
                    words.push(a.clone());
                    continue;
                }
                let today = now.date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
                let midnight = |d: chrono::NaiveDateTime| {
                    d.and_local_timezone(chrono::Local)
                        .earliest()
                        .map_or(0, |t| t.timestamp())
                };
                since = match rest.next().map(String::as_str) {
                    Some("today") => midnight(today),
                    Some("yesterday") => midnight(today - chrono::Duration::days(1)),
                    Some("week") => (now - chrono::Duration::days(7)).timestamp(),
                    Some("month") => (now - chrono::Duration::days(30)).timestamp(),
                    Some(n) if n.ends_with('d') && n[..n.len() - 1].parse::<i64>().is_ok() => {
                        let days: i64 = n[..n.len() - 1].parse().unwrap_or(0);
                        (now - chrono::Duration::days(days.clamp(0, 3650))).timestamp()
                    }
                    _ => {
                        eprintln!("usage: friday-ctl answers search <words…> [--since today|yesterday|week|month|<N>d]");
                        return ExitCode::from(2);
                    }
                };
            }
            let topic = words.join(" ");
            (timeline::search_words(&topic), since, topic)
        }
        _ => {
            eprintln!("usage: friday-ctl answers search <words…> [--since …] | answers told <ask>");
            return ExitCode::from(2);
        }
    };
    if words.is_empty() {
        eprintln!("Say what to look for: friday answers search <words>");
        return ExitCode::from(2);
    }
    let found = timeline::search_answers(&text, &words, since, true, 10);
    if found.is_empty() {
        println!("Nothing in past answers about {topic}.");
    } else {
        println!(
            "Past answers about {topic}:\n{}",
            timeline::render_found(&found)
        );
    }
    ExitCode::SUCCESS
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
    // "-": the text is read as it comes, so an answer piped in while it
    // streams is spoken from its first sentence.
    let said = if args == ["-"] {
        friday_mobile::speech::say_reader(std::io::stdin())
    } else {
        friday_mobile::speech::say(&args.join(" "))
    };
    match said {
        // Stopped before the end (friday say --stop, the talk key, talking
        // over it): its own exit code, so nothing listens for a follow-up.
        Ok(spoken) if spoken.stopped => std::process::ExitCode::from(5),
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("friday-ctl: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    friday_mobile::secrets::resolve_env();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("models") {
        return models(&raw[1..]).await;
    }
    if raw.first().map(String::as_str) == Some("say") {
        return say(&raw[1..]);
    }
    if raw.first().map(String::as_str) == Some("watch") {
        return watch(&raw[1..]);
    }
    // `friday-ctl doc-text <file> [max-bytes]`: a document's text, for asks.
    if raw.first().map(String::as_str) == Some("doc-text") {
        let Some(path) = raw.get(1) else {
            eprintln!("usage: friday-ctl doc-text <file> [max-bytes]");
            return std::process::ExitCode::from(2);
        };
        let max = raw.get(2).and_then(|m| m.parse().ok()).unwrap_or(8000);
        return match friday_mobile::local_docs::doc_text(std::path::Path::new(path), max) {
            Some(text) => {
                println!("{text}");
                std::process::ExitCode::SUCCESS
            }
            None => std::process::ExitCode::from(1),
        };
    }
    if raw.first().map(String::as_str) == Some("recall") {
        return recall(&raw[1..]).await;
    }
    if raw.first().map(String::as_str) == Some("window-text") {
        return window_text(&raw[1..]).await;
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
            Some("hold") => std::process::ExitCode::from(friday_mobile::listen::run_hold()),
            Some("answer") => std::process::ExitCode::from(friday_mobile::listen::run_answer()),
            Some("barge") => std::process::ExitCode::from(friday_mobile::listen::run_barge()),
            Some("setup") => {
                match friday_mobile::listen::setup(&friday_mobile::listen::whisper_dir()) {
                    Ok(()) => {
                        println!("Voice is set up. Hold the ask key and speak.");
                        std::process::ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("friday-ctl: {e}");
                        std::process::ExitCode::from(1)
                    }
                }
            }
            _ => {
                eprintln!(
                    "usage: friday-ctl listen captions [--mic] | wake | hold | answer | barge | setup"
                );
                std::process::ExitCode::from(2)
            }
        };
    }
    if raw.first().map(String::as_str) == Some("local-ai")
        && raw.get(1).map(String::as_str) == Some("install-llamacpp")
    {
        let Some(model) = raw.get(2) else {
            eprintln!(
                "usage: friday-ctl local-ai install-llamacpp <model|file.gguf> [--gpu|--cpu]"
            );
            return std::process::ExitCode::from(2);
        };
        // The same "progress" lines as pull; "done vision=0|1" at the end.
        // Neither: the build already set up (a "Use" of another model).
        let gpu = if raw.iter().any(|a| a == "--gpu") {
            Some(true)
        } else if raw.iter().any(|a| a == "--cpu") {
            Some(false)
        } else {
            None
        };
        let result = friday_mobile::local_ai::install_llamacpp(model, gpu, |e| {
            use std::io::Write;
            if let friday_mobile::local_ai::PullEvent::Progress(p, d, t) = e {
                println!("progress {p} {d} {t}");
            }
            let _ = std::io::stdout().flush();
        })
        .await;
        return match result {
            Ok(vision) => {
                println!("done vision={}", u8::from(vision));
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    if raw.first().map(String::as_str) == Some("local-ai")
        && raw.get(1).map(String::as_str) == Some("pull")
    {
        let Some(model) = raw.get(2) else {
            eprintln!("usage: friday-ctl local-ai pull <model>");
            return std::process::ExitCode::from(2);
        };
        // "progress <percent> <MB in> <MB in all>" lines, then "done".
        let result = friday_mobile::local_ai::ollama_pull(model, |e| {
            use std::io::Write;
            match e {
                friday_mobile::local_ai::PullEvent::Progress(p, d, t) => {
                    println!("progress {p} {d} {t}")
                }
                friday_mobile::local_ai::PullEvent::Done => println!("done"),
                _ => {}
            }
            let _ = std::io::stdout().flush();
        })
        .await;
        return match result {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    if raw.first().map(String::as_str) == Some("local-ai")
        && raw.get(1).map(String::as_str) == Some("engines")
    {
        return local_ai_engines(raw.iter().any(|a| a == "--json")).await;
    }
    if raw.first().map(String::as_str) == Some("local-ai") {
        return local_ai(&raw[1..]);
    }
    // `docs ... -`, `show ... -` and `browser find -`: the question comes on
    // stdin, so it isn't on the command line for every program to read.
    let mut raw = raw;
    let browser_find = raw.first().map(String::as_str) == Some("browser")
        && raw.get(1).map(String::as_str) == Some("find");
    if (browser_find || matches!(raw.first().map(String::as_str), Some("docs" | "show")))
        && raw.last().map(String::as_str) == Some("-")
    {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
        *raw.last_mut().unwrap() = text.trim_end_matches('\n').to_string();
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
            || (a.contains("org.friday.browser") && a.ends_with(".json"))
    });
    if started_by_browser {
        // On a thread of its own: main is already inside a Tokio runtime, and
        // the Windows host starts its own (a nested block_on panics, so Brave
        // saw "Native host has exited").
        let code = std::thread::spawn(friday_mobile::browser_host::run_host)
            .join()
            .unwrap_or(1);
        return std::process::ExitCode::from(code);
    }
    // `friday-ctl browser find <question>`: ring what it names on the page in front.
    if raw.first().map(String::as_str) == Some("browser")
        && raw.get(1).map(String::as_str) == Some("find")
    {
        return match friday_mobile::browser_host::find(&raw[2..].join(" ")) {
            Ok(v) if v.get("found").and_then(serde_json::Value::as_bool) == Some(true) => {
                println!("{v}");
                std::process::ExitCode::SUCCESS
            }
            Ok(v) => {
                println!("{v}");
                std::process::ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::FAILURE
            }
        };
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
    // `friday-ctl import [--apply]`: keys and a model from other assistants,
    // previewed (keys shown by their last 4 characters), then added.
    if raw.first().map(String::as_str) == Some("import") {
        use friday_mobile::import;
        let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        else {
            return std::process::ExitCode::FAILURE;
        };
        let found = import::find_all(std::path::Path::new(&home));
        let env_path = friday_mobile::backup::folders()[0].1.join("friday.env");
        let env = std::fs::read_to_string(&env_path).unwrap_or_default();
        print!("{}", import::preview(&found, &env));
        if raw.get(1).map(String::as_str) != Some("--apply") {
            if import::apply(&found, &env).1 > 0 {
                println!("Nothing was changed. Add these: friday import --apply");
            }
            return std::process::ExitCode::SUCCESS;
        }
        let (text, added) = import::apply(&found, &env);
        if added == 0 {
            println!("Nothing new to add.");
            return std::process::ExitCode::SUCCESS;
        }
        let tmp = env_path.with_extension("env.tmp");
        let written = env_path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let mut opts = std::fs::OpenOptions::new();
                opts.write(true).create(true).truncate(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
                std::io::Write::write_all(&mut opts.open(&tmp)?, text.as_bytes())
            })
            .and_then(|()| std::fs::rename(&tmp, &env_path));
        return match written {
            Ok(()) => {
                println!("Added {added}.");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: couldn't write {}: {e}", env_path.display());
                std::process::ExitCode::FAILURE
            }
        };
    }
    // `friday-ctl mask [--] <text…>`: the text with keys and passwords
    // masked, for history the hosts keep (one rule for both).
    if raw.first().map(String::as_str) == Some("mask") {
        // `mask -`: all of stdin (a whole answer), printed as it came.
        if raw.get(1).map(String::as_str) == Some("-") {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
            print!("{}", friday_goal_execution::mask_secrets(&text).0);
            return std::process::ExitCode::SUCCESS;
        }
        let words: Vec<&str> = raw[1..]
            .iter()
            .map(String::as_str)
            .skip_while(|w| *w == "--")
            .collect();
        println!(
            "{}",
            friday_goal_execution::mask_secrets(&words.join(" ")).0
        );
        return std::process::ExitCode::SUCCESS;
    }
    if raw.first().map(String::as_str) == Some("answers") {
        return answers(&raw[1..], &friday_intelligence::default_state_dir());
    }
    if raw.first().map(String::as_str) == Some("day") {
        return day(&raw[1..], &friday_intelligence::default_state_dir());
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
    // "What did I miss?": from FRIDAY's own state, nothing sent anywhere.
    if raw.first().map(String::as_str) == Some("brief") {
        if !friday_mobile::features::on("brief") {
            println!("The brief is turned off (friday feature on brief).");
            return std::process::ExitCode::SUCCESS;
        }
        let state = friday_mobile::shell_state::ShellState::default_path();
        let dir = state.parent().unwrap_or(std::path::Path::new("."));
        let now = chrono::Local::now().timestamp();
        let since = friday_mobile::brief::since(dir, now);
        let said =
            friday_mobile::brief::text(dir, &friday_mobile::standing::store_path(), since, now);
        println!("{said}");
        let _ = friday_mobile::brief::mark_seen(dir, now);
        if raw.iter().any(|a| a == "--say") {
            let _ = friday_mobile::speech::say(&said);
        }
        return std::process::ExitCode::SUCCESS;
    }
    // A checked backup of settings, memory and history, its check, and a
    // restore that checks first and keeps the current files aside.
    if raw.first().map(String::as_str) == Some("backup") {
        if !friday_mobile::features::on("backup") {
            println!("Backups are turned off (Settings, Features: backup).");
            return std::process::ExitCode::SUCCESS;
        }
        use friday_mobile::backup;
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        let said = match (raw.get(1).map(String::as_str), raw.get(2)) {
            (Some("check"), Some(dir)) => backup::check(std::path::Path::new(dir)).map(|m| {
                format!(
                    "The backup is whole: {} files, made {}.",
                    m.files.len(),
                    m.created
                )
            }),
            (Some("restore"), Some(dir)) => {
                let dir = std::path::Path::new(dir);
                backup::restore(dir, &backup::folders(), &stamp).map(|n| {
                    // Made on another computer: saved folders now name this home.
                    let moved = match backup::this_home() {
                        Some(home) if friday_mobile::features::on("backup-new-home") => {
                            backup::rehome(dir, &backup::folders(), &home)
                        }
                        _ => Ok(0),
                    };
                    let moved = match moved {
                        Ok(0) => String::new(),
                        Ok(n) => format!(" Saved folders in {n} files now point at this computer's home."),
                        Err(e) => format!(" Saved folders couldn't all be pointed at this computer's home ({e})."),
                    };
                    format!(
                        "Restored {n} files. What they replaced is in before-restore-{stamp} in FRIDAY's folders, and your API keys were kept.{moved} Restart FRIDAY to use them."
                    )
                })
            }
            (Some("check" | "restore"), None) => {
                Err("usage: friday-ctl backup [folder] | check <folder> | restore <folder>".into())
            }
            (folder, _) => {
                let parent = folder
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(backup::default_parent);
                backup::create(&backup::folders(), &parent, &stamp).map(|(dir, n)| {
                    format!(
                        "Backed up {n} files to {} and checked them. API keys and downloaded voices aren't in it.",
                        dir.display()
                    )
                })
            }
        };
        return match said {
            Ok(s) => {
                println!("{s}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    // What FRIDAY may do on its own: every tool action and whether it runs,
    // is watched, asks first or never runs.
    if raw.first().map(String::as_str) == Some("risks") {
        if !friday_mobile::features::on("risk-table") {
            println!("The risk table is turned off (Settings, Features: risk-table).");
            return std::process::ExitCode::SUCCESS;
        }
        println!("{}", friday_mobile::risk_table::report());
        return std::process::ExitCode::SUCCESS;
    }
    // Whether the action log is as FRIDAY wrote it (each line chained to
    // the one before).
    if raw.first().map(String::as_str) == Some("verify-log") {
        let Some(dir) = friday_mobile::desktop_tool::audit_dir() else {
            eprintln!("friday-ctl: no state folder");
            return std::process::ExitCode::from(1);
        };
        return match friday_mobile::action_log::verify(&dir) {
            Ok(s) => {
                println!("{s}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                println!("Changed: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    // The current conversation as a document (friday phone send thread).
    if raw.first().map(String::as_str) == Some("thread") {
        let state_path = std::env::var("FRIDAY_STATE_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| ShellState::default_path());
        let id = std::fs::read_to_string(state_path.with_file_name("conversation"))
            .ok()
            .and_then(|t| t.split_whitespace().next().map(str::to_string))
            .unwrap_or_default();
        return match friday_mobile::goal_runner::thread_markdown(
            &state_path.with_file_name("conversations.json"),
            &id,
        ) {
            Some(md) => {
                print!("{md}");
                std::process::ExitCode::SUCCESS
            }
            None => {
                eprintln!("friday-ctl: there's no conversation to send yet");
                std::process::ExitCode::from(1)
            }
        };
    }
    // Agent steps waiting for an answer, runs going now, standing tasks and
    // runs finished lately, in one list.
    if raw.first().map(String::as_str) == Some("activity") {
        if !friday_mobile::features::on("activity") {
            println!("The activity list is turned off (Settings, Features: activity).");
            return std::process::ExitCode::SUCCESS;
        }
        println!(
            "{}",
            friday_mobile::activity::report(
                &friday_mobile::agent_runs::runs_dir(),
                &friday_mobile::standing::store_path(),
                chrono::Utc::now().timestamp()
            )
        );
        return std::process::ExitCode::SUCCESS;
    }
    // How each AI provider has done lately, from the local egress log.
    // How long each stage of an ask took lately (median and p95).
    if raw.first().map(String::as_str) == Some("timings") {
        let log =
            std::fs::read_to_string(friday_intelligence::default_egress_path()).unwrap_or_default();
        let since = chrono::Utc::now().timestamp() - 7 * 86400;
        println!(
            "{}",
            friday_mobile::timings::report(&friday_mobile::timings::read(), &log, since, 7)
        );
        return std::process::ExitCode::SUCCESS;
    }
    // How long each kind of record stays.
    if raw.first().map(String::as_str) == Some("retention") {
        println!("{}", friday_mobile::retention::describe());
        return std::process::ExitCode::SUCCESS;
    }
    // A fact forgotten from every place it was copied to.
    if raw.first().map(String::as_str) == Some("forget-everywhere") {
        let words = raw[1..]
            .iter()
            .filter(|a| *a != "--daemon-stopped")
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let state_path = ShellState::default_path();
        // A running daemon holds conversations in memory and would write
        // them back: it stops first (friday memory forget does that, and
        // says so with --daemon-stopped).
        let running = std::fs::read_to_string(&state_path).is_ok_and(|s| {
            s.contains("\"status\": \"running\"") || s.contains("\"status\":\"running\"")
        });
        if running && raw.iter().all(|a| a != "--daemon-stopped") {
            eprintln!("friday-ctl: stop FRIDAY first (friday memory forget does it for you)");
            return std::process::ExitCode::from(1);
        }
        let state = state_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let places = friday_mobile::forget::Places {
            memory: friday_mobile::shell_state::memory_dir(
                std::env::var("FRIDAY_MEMORY_DIR").ok(),
                &state_path,
                std::path::Path::new("friday-data"),
            ),
            state,
            proposal: friday_mobile::about::tidy_path(),
        };
        return match friday_mobile::forget::everywhere(&places, &words) {
            Ok(done) if done.is_empty() => {
                println!("No other copy had \"{words}\".");
                std::process::ExitCode::SUCCESS
            }
            Ok(done) => {
                let list: Vec<String> = done.iter().map(|(w, n)| format!("{n} from {w}")).collect();
                println!(
                    "Also forgot it everywhere it was copied: {}.",
                    list.join(", ")
                );
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(2)
            }
        };
    }
    if raw.first().map(String::as_str) == Some("usage") {
        let days: u32 = raw
            .iter()
            .position(|a| a == "--days")
            .and_then(|i| raw.get(i + 1))
            .and_then(|d| d.parse().ok())
            .filter(|d| (1..=365).contains(d))
            .unwrap_or(7);
        let log =
            std::fs::read_to_string(friday_intelligence::default_egress_path()).unwrap_or_default();
        let since = chrono::Utc::now().timestamp() - i64::from(days) * 86400;
        println!("{}", friday_mobile::usage::report(&log, since, days));
        return std::process::ExitCode::SUCCESS;
    }
    // Another assistant's notes into memory.md: a preview first, --apply adds.
    if raw.first().map(String::as_str) == Some("memory") {
        let done = if raw.get(1).map(String::as_str) == Some("tidy") {
            memory_tidy(&raw[2..])
        } else {
            memory_import(&raw[1..])
        };
        return match done {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(2)
            }
        };
    }
    // Proof that nothing left: the egress log and the daemon's connections.
    if raw.first().map(String::as_str) == Some("offline-proof") {
        let hours: i64 = raw
            .get(1)
            .and_then(|h| h.parse().ok())
            .unwrap_or(24)
            .clamp(1, 24 * 30);
        let state_path = std::env::var("FRIDAY_STATE_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| ShellState::default_path());
        let dir = state_path.parent().unwrap_or(std::path::Path::new("."));
        let egress = std::fs::read_to_string(dir.join("egress.jsonl")).ok();
        let private = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["private"].as_bool())
            .unwrap_or(false);
        let since = chrono::Utc::now() - chrono::Duration::hours(hours);
        let agent_runs = friday_mobile::agent_runs::list(&friday_mobile::agent_runs::runs_dir())
            .iter()
            .filter(|r| r.started >= since.timestamp())
            .count();
        let conns = friday_mobile::offline_proof::daemon_connections();
        println!(
            "{}",
            friday_mobile::offline_proof::report(
                since,
                hours,
                &friday_mobile::offline_proof::Seen {
                    egress: egress.as_deref(),
                    private,
                    connections: conns.as_deref(),
                    agent_runs,
                    covers: friday_mobile::offline_proof::COVERS,
                },
            )
        );
        return std::process::ExitCode::SUCCESS;
    }
    // Ready-made standing tasks: listed, or one added by name.
    if raw.first().map(String::as_str) == Some("tasks") {
        use friday_mobile::standing;
        let said = match (raw.get(1).map(String::as_str), raw.get(2)) {
            (Some("recipes"), _) => Ok(standing::RECIPES
                .iter()
                .map(|r| format!("{}: {}", r.id, r.what))
                .collect::<Vec<_>>()
                .join("\n")
                + "\nAdd one: friday tasks recipe <name>"),
            (Some("recipe"), Some(id)) => {
                standing::add_recipe(&standing::store_path(), id, chrono::Utc::now().timestamp())
            }
            _ => Err("usage: friday-ctl tasks recipes | recipe <name>".into()),
        };
        return match said {
            Ok(s) => {
                println!("{s}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }
    // `friday undo` of a file FRIDAY wrote: put back while it's unchanged.
    if raw.first().map(String::as_str) == Some("undo-file") {
        let (Some(path), Some(copy), Some(written), Some(dir)) = (
            raw.get(1),
            raw.get(2),
            raw.get(3),
            friday_mobile::desktop_tool::audit_dir(),
        ) else {
            eprintln!("usage: friday-ctl undo-file <path> <kept copy or \"\"> <hash>");
            return std::process::ExitCode::from(2);
        };
        return match friday_mobile::file_undo::undo(&dir, std::path::Path::new(path), copy, written)
        {
            Ok(s) => {
                println!("{s}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                println!("Can't undo that: {e}.");
                std::process::ExitCode::from(1)
            }
        };
    }
    // A run's separate copy: brought in (staged) or dropped.
    if raw.first().map(String::as_str) == Some("agents") {
        let dir = friday_mobile::agent_runs::runs_dir();
        let done = match (raw.get(1).map(String::as_str), raw.get(2)) {
            (Some("accept"), Some(id)) => friday_mobile::agent_runs::accept(&dir, id),
            (Some("discard"), Some(id)) => friday_mobile::agent_runs::discard(&dir, id),
            (Some("compare"), Some(id)) => friday_mobile::agent_runs::compare(&dir, id),
            _ => Err("usage: friday-ctl agents accept|discard|compare <id>".into()),
        };
        return match done {
            Ok(said) => {
                println!("{said}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("friday-ctl: {e}");
                std::process::ExitCode::from(1)
            }
        };
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
    let mut json = false;
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
    let mut picture = false;
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
        } else if w == "--picture" {
            picture = true;
            i += 1;
        } else if w == "--wait" {
            wait = true;
            i += 1;
        } else if w == "--json" {
            // The stable form for scripts (specs/ask-json.md).
            json = true;
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
        } else if w == "--scheduled" {
            // A standing task asking on its own: only what can be undone runs.
            ask_opts.insert("scheduled".into(), serde_json::json!(true));
            i += 1;
        } else if w == "--typed" {
            // Typed with nothing added: a public free tier may answer (LAI-6).
            ask_opts.insert("typed".into(), serde_json::json!(true));
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
        eprintln!("usage: friday-ctl <goal|cancel|system-info|context screen> [goal text] [--json] [--web] [--thorough [--avoid <provider>/<model>]] [--new-conversation] [--addr <host:port>] [--token <tok>]");
        return std::process::ExitCode::from(if json { 2 } else { 1 });
    };
    let sub = sub.clone();
    let mut goal_words = positional.into_iter().skip(1).collect::<Vec<_>>();
    // `goal -`: the question comes on stdin, so it (and any selection or page
    // text with it) isn't on the command line for every program to read.
    if sub == "goal" && goal_words.len() == 1 && goal_words[0] == "-" {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
        goal_words = vec![text.trim_end_matches('\n').to_string()];
    }
    if json {
        // The contract (specs/ask-json.md) is for a question only, and its
        // one object leaves no room for {"partial": ...} lines.
        if sub != "goal" || goal_words.join(" ").trim().is_empty() {
            eprintln!("usage: friday-ctl goal --json [--web] [--new-conversation] <question>");
            return std::process::ExitCode::from(2);
        }
        stream = false;
    }
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
                let said = format!(
                    "no token found; FRIDAY isn't running (no {}): {hint}",
                    state_path.display()
                );
                if json && sub == "goal" {
                    return json_unreachable(&said);
                }
                eprintln!("{said}");
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
        // The conversation `goal` asks keep: a screen ask joins it, so
        // "how do I fix it?" next knows what was on screen.
        let conversation = conversation_id(
            &state_path.with_file_name("conversation"),
            new_conversation,
            now_ms(),
        );
        return run_context(
            &addr_str,
            &cmd_token,
            question.as_deref(),
            region,
            picture,
            &conversation,
            stream,
        )
        .await;
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
        json,
    )
    .await;
    if sub == "goal" && wait && accepted == std::process::ExitCode::SUCCESS {
        return wait_for_goal(&state_path, &goal, submitted_ms, stream, json).await;
    }
    accepted
}

/// `--json`'s exit codes (specs/ask-json.md): the contract scripts rely on.
const JSON_FAILED: u8 = 1;
const JSON_NEEDS_CONFIRMATION: u8 = 3;
const JSON_TIMED_OUT: u8 = 4;
const JSON_UNREACHABLE: u8 = 5;

fn json_unreachable(error: &str) -> std::process::ExitCode {
    println!(
        "{}",
        serde_json::json!({ "outcome": "unreachable", "result": null, "error": error })
    );
    std::process::ExitCode::from(JSON_UNREACHABLE)
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
    json: bool,
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
                // The contract promises one of its words, never null.
                "outcome": done["outcome"].as_str().unwrap_or("failed"),
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
            // --json: one line, so a script reads the last line.
            let shown = if json {
                summary.to_string()
            } else {
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            };
            println!("{shown}");
            return if done["outcome"] == "completed" {
                std::process::ExitCode::SUCCESS
            } else if done
                .get("needs_confirmation")
                .is_some_and(|nc| !nc.is_null())
            {
                std::process::ExitCode::from(JSON_NEEDS_CONFIRMATION)
            } else {
                std::process::ExitCode::from(JSON_FAILED)
            };
        }
        let poll = if stream { 100 } else { 300 };
        tokio::time::sleep(Duration::from_millis(poll)).await;
    }
    let said = format!("timed out after {limit}s waiting for the goal to finish");
    if json {
        println!(
            "{}",
            serde_json::json!({ "outcome": "timed_out", "result": null, "error": said })
        );
        return std::process::ExitCode::from(JSON_TIMED_OUT);
    }
    eprintln!("{said}");
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
    json: bool,
) -> std::process::ExitCode {
    // Build the command. Only `goal`, `system-info`, and `context` are wired
    // for now — the "one controlled command" surface.
    let (method, params) = match sub {
        "goal" => {
            let goal = args.join(" ");
            if goal.trim().is_empty() {
                eprintln!("friday-ctl goal <text>: goal text required");
                return std::process::ExitCode::from(if json { 2 } else { 1 });
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
    if !json {
        return send_command(addr, &method, params, Duration::from_secs(10), false).await;
    }
    // --json: the taking-it reply stays off stdout; one object comes at the end.
    match exchange(addr, &method, params, Duration::from_secs(10), false).await {
        Ok((true, _)) => std::process::ExitCode::SUCCESS,
        Ok((false, result)) => {
            let error = match result.get("error").unwrap_or(&result) {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            println!(
                "{}",
                serde_json::json!({ "outcome": "rejected", "result": null, "error": error })
            );
            std::process::ExitCode::from(JSON_FAILED)
        }
        Err(e) => json_unreachable(&e),
    }
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
    picture: bool,
    conversation: &str,
    stream: bool,
) -> std::process::ExitCode {
    let mut params =
        serde_json::json!({ "token": token, "consent": true, "conversation": conversation });
    if let Some(q) = question {
        params["question"] = serde_json::Value::String(q.to_string());
    }
    if let Some(r) = region {
        params["region"] = r;
    }
    if picture {
        params["picture"] = serde_json::Value::Bool(true);
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
    progress_field(msg, id, "partial")
}

/// What a `context.progress` event says left for the model, for command `id`.
fn context_receipt(msg: &ServerMessage, id: &str) -> Option<String> {
    progress_field(msg, id, "receipt")
}

fn progress_field(msg: &ServerMessage, id: &str, field: &str) -> Option<String> {
    match msg {
        ServerMessage::Event { topic, payload }
            if topic == "context.progress" && payload["id"] == id =>
        {
            payload[field].as_str().map(str::to_string)
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
    match exchange(addr, method, params, wait, stream).await {
        Ok((ok, result)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
            if ok {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Sends one command and returns the daemon's answer (whether it took it, and
/// its result), or why it couldn't be reached.
async fn exchange(
    addr: &str,
    method: &str,
    params: serde_json::Value,
    wait: Duration,
    stream: bool,
) -> Result<(bool, serde_json::Value), String> {
    use std::io::Write;
    let ws_url = format!("ws://{addr}");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("cannot reach daemon at {ws_url}: {e}"))?;

    let id = uuid::Uuid::new_v4().to_string();
    let want_id = id.clone();

    // In open mode the server accepts a command directly; send ours and read
    // the matching CommandResult. Any welcome/events are consumed first.
    let client_msg = ClientMessage::Command {
        id: id.clone(),
        method: method.to_string(),
        params,
    };
    ws.send(Message::Text(serde_json::to_string(&client_msg).unwrap()))
        .await
        .map_err(|e| format!("couldn't send the command to the daemon: {e}"))?;

    // Loop until we see our CommandResult (skipping welcome/events).
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err("timed out waiting for command result".into());
        }
        let msg = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => return Err(format!("ws error: {e}")),
            Ok(None) => {
                return Err(
                    "the daemon closed the connection before it answered (is it restarting?)"
                        .into(),
                )
            }
            // Nothing for a while: keep waiting until the deadline.
            Err(_) => continue,
        };
        if let Message::Text(t) = msg {
            let decoded = decode_server(&t);
            if let (true, Ok(m)) = (stream, &decoded) {
                let line = context_progress(m, &want_id)
                    .map(|partial| serde_json::json!({ "partial": partial }))
                    .or_else(|| {
                        context_receipt(m, &want_id)
                            .map(|receipt| serde_json::json!({ "receipt": receipt }))
                    });
                if let Some(line) = line {
                    let mut out = std::io::stdout().lock();
                    let _ = writeln!(out, "{line}");
                    let _ = out.flush();
                    continue;
                }
            }
            if let Ok(ServerMessage::CommandResult { id, ok, result }) = decoded {
                if id == want_id {
                    return Ok((ok, result));
                }
            }
        }
    }
}

/// `memory import <file> [--apply]`: what another assistant's notes would add
/// to memory.md, and with --apply, adds it.
/// `friday-ctl memory tidy [apply]`: when the memory file holds more than an
/// ask carries, the person's AI provider proposes a shorter list (the facts
/// go as quoted text: no action can come of it); the proposal is shown and
/// kept, and `apply` puts exactly it in place, the old file kept beside it.
fn memory_tidy(args: &[String]) -> Result<(), String> {
    use friday_mobile::about;
    if !friday_mobile::features::on("memory-file") || !friday_mobile::features::on("memory-tidy") {
        return Err("tidying memory is turned off (friday feature memory-tidy on)".into());
    }
    let dest = about::path();
    let text = about::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let have = about::facts(&text);
    let proposal = about::tidy_path();
    if args.first().map(String::as_str) == Some("apply") {
        let tidy = std::fs::read_to_string(&proposal).map_err(|_| {
            "there's no proposal to apply: run friday memory tidy first".to_string()
        })?;
        let new = about::facts(&tidy);
        if new.is_empty() {
            return Err("the proposal has no facts in it; nothing was changed".into());
        }
        let keep = dest.with_file_name("memory.before-tidy.md");
        std::fs::copy(&dest, &keep).map_err(|e| format!("{}: {e}", keep.display()))?;
        let tmp = dest.with_extension("md.tmp");
        std::fs::write(&tmp, about::with_facts(&text, &new))
            .and_then(|()| std::fs::rename(&tmp, &dest))
            .map_err(|e| format!("{}: {e}", dest.display()))?;
        let _ = std::fs::remove_file(&proposal);
        println!(
            "Memory now holds {} facts (was {}). The old file is {}.",
            new.len(),
            have.len(),
            keep.display()
        );
        return Ok(());
    }
    if !about::too_long(&have) {
        println!(
            "Everything fits: {} facts, all carried by every ask. Nothing to tidy.",
            have.len()
        );
        return Ok(());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new(me)
        .args([
            "goal",
            "--wait",
            "--new-conversation",
            &about::tidy_prompt(&have),
        ])
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let text_out = String::from_utf8_lossy(&out.stdout);
    let last = serde_json::Deserializer::from_str(&text_out)
        .into_iter::<serde_json::Value>()
        .filter_map(Result::ok)
        .filter(|v| v.get("outcome").is_some())
        .last()
        .unwrap_or_default();
    let answer = match (last["outcome"].as_str(), last["result"].as_str()) {
        (Some("completed"), Some(a)) => a.to_string(),
        _ => {
            return Err(last["error"]
                .as_str()
                .unwrap_or("FRIDAY didn't answer (is it running?)")
                .to_string())
        }
    };
    let new = about::facts(&answer);
    if new.is_empty() {
        return Err("the model didn't give a list; nothing was proposed".into());
    }
    let body = new
        .iter()
        .map(|f| format!("- {f}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&proposal, &body).map_err(|e| format!("{}: {e}", proposal.display()))?;
    println!(
        "Proposed: {} facts instead of {} (check nothing was lost):\n{body}",
        new.len(),
        have.len()
    );
    if about::too_long(&new) {
        println!("It's still more than an ask carries; trim it after.");
    }
    println!("Nothing changed yet. friday memory tidy apply puts this in place.");
    Ok(())
}

fn memory_import(args: &[String]) -> Result<(), String> {
    use friday_mobile::about;
    let file = args.iter().skip(1).find(|a| !a.starts_with("--"));
    let (Some("import"), Some(file)) = (args.first().map(String::as_str), file) else {
        return Err("usage: friday-ctl memory import <file> [--apply]".into());
    };
    if !friday_mobile::features::on("memory-file") {
        return Err("the memory file is turned off (friday feature on memory-file)".into());
    }
    let apply = args.iter().any(|a| a == "--apply");
    let meta = std::fs::metadata(file).map_err(|e| format!("{file}: {e}"))?;
    if meta.len() > 1_000_000 {
        return Err(format!("{file} is over 1 MB: not a notes file"));
    }
    let notes = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let dest = about::path();
    let have = about::facts(&about::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?);
    let new = about::importable(&notes, &have);
    if new.is_empty() {
        println!("Nothing new to bring in from {file}.");
        return Ok(());
    }
    let crowded = about::too_long(&[have.clone(), new.clone()].concat());
    if !apply {
        println!("Would add {} line(s) to {}:", new.len(), dest.display());
        // Worked out as --apply will do it (a later line can replace an earlier one).
        let text = about::read(&dest).unwrap_or_default();
        let (_, done) = about::apply(&text, &new, "today");
        for (f, closed) in &done {
            if closed.is_empty() {
                println!("- {f}");
            } else {
                println!("- {f}   (replaces: {})", closed.join("; "));
            }
        }
        if crowded {
            println!("That is more than an ask carries: later lines would be left out. Trim the file after.");
        }
        println!("Nothing was added. Run again with --apply to add them.");
        return Ok(());
    }
    let n = about::remember_all(&new).map_err(|e| format!("{}: {e}", dest.display()))?;
    println!("Added {n} line(s) to {}.", dest.display());
    if crowded {
        println!("It is now more than an ask carries: later lines are left out. Trim the file.");
    }
    Ok(())
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
        let receipt = ServerMessage::Event {
            topic: "context.progress".to_string(),
            payload: json!({"id": "c1", "receipt": "firefox · the window's text, 2.1k chars"}),
        };
        assert_eq!(
            context_receipt(&receipt, "c1").as_deref(),
            Some("firefox · the window's text, 2.1k chars")
        );
        assert_eq!(context_progress(&receipt, "c1"), None);
        assert_eq!(context_receipt(&ev("c1"), "c1"), None);
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
