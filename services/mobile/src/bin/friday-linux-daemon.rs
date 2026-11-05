//! FRIDAY Linux Daemon (R5-1-1) — the goal-owning Linux-graphical service that
//! bridges the Omarchy Quickshell desktop to the FRIDAY runtime.
//!
//! ```text
//! FRIDAY runtime (EventBus)
//!      ├─ SystemProbe ──▶ telemetry.system events
//!      ├─ GoalRunner   ──▶ runs real Executive over real evidence adapters
//!      └─ shell_state  ──▶ ~/.local/state/friday/state.json  (file-backed contract)
//!             │
//!   WsServer (127.0.0.1:18432, loopback, token-gated) ◀── command ingress
//!             │  CommandGateway ──▶ plan.goal / task.* / system.info
//! ```
//!
//! The thin vertical slice that proves the two worlds live together:
//! boot → daemon starts → shell discovers `friday.shell` → shell renders live
//! state from `state.json` → shell issues one controlled command
//! (`planner.goal`) → the daemon runs it. No new IPC framework, no second event
//! bus, no second shell architecture. Reuses `GoalRunner`, `Executive`, the
//! evidence adapters, `WsServer`, `CommandGateway`, `SystemProbe`, and
//! `VoiceIntake` exactly as the mobile daemon wires them, and adds only the
//! Linux-graphical pieces that were greenfield: goal ownership, `state.json`,
//! SIGTERM shutdown, and systemd readiness.

use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use friday_context_collector::{ContextCollector, RealCommandRunner};
use friday_core::runtime::Friday;
use friday_goal_execution::MemoryJournal;
use friday_intelligence::InferenceFabric;
use friday_mobile::shell_state::{CommandRecord, ShellState};
use friday_mobile::{CommandGateway, GoalRunner, SystemProbe, VoiceConfig, VoiceIntake, WsServer};
use tokio::sync::broadcast;
use tokio::sync::Mutex;

/// Default loopback address for the shell ingress. Intentional: the coarse
/// trust boundary is the loopback bind itself (never `0.0.0.0` for the shell).
const DEFAULT_LISTEN: &str = "127.0.0.1:18432";

/// Emit sd_notify readiness/stopping to systemd without a crate dependency.
/// `$NOTIFY_SOCKET` carries an AF_UNIX datagram socket; `READY=1` is enough for
/// a `Type=notify` user service. No socket = no-op (running under a bare
/// shell, not systemd).
#[cfg(unix)]
fn sd_notify(msg: &str) {
    let Ok(sock) = std::env::var("NOTIFY_SOCKET") else {
        return;
    };
    if sock.is_empty() {
        return;
    }
    use std::os::unix::net::UnixDatagram;
    let Ok(s) = UnixDatagram::unbound() else {
        return;
    };
    let _ = s.connect(&sock);
    let _ = s.send(msg.as_bytes());
}

#[cfg(not(unix))]
fn sd_notify(_msg: &str) {}

/// Generate a per-run session token the shell must present on command ingress.
fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Atomically write the state document (tmp + rename) so any inotify watch
/// never observes a half-written file. Owner-only (0600): it carries the
/// session token that authorizes commands.
fn write_state(path: &std::path::Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = match std::fs::File::create(&tmp) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("state: cannot open tmp {}: {e}", tmp.display());
            return;
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = f.set_permissions(std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!("state: cannot restrict {}: {e}", tmp.display());
            return;
        }
    }
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("state: serialize failed: {e}");
            return;
        }
    };
    if let Err(e) = f.write_all(&bytes) {
        tracing::warn!("state: write failed: {e}");
        return;
    }
    if let Err(e) = f.sync_all() {
        tracing::warn!("state: sync failed: {e}");
    }
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, path) {
        tracing::warn!("state: rename failed: {e}");
    }
}

fn render(state: &ShellState) -> serde_json::Value {
    state.to_json(now_ms(), now_utc())
}

#[tokio::main]
async fn main() {
    friday_core::init_telemetry();
    tracing::info!("FRIDAY Linux daemon starting (R5-1-1)");

    // Graceful-shutdown flag shared with the signal handler and the WS loop.
    let stopping = Arc::new(AtomicBool::new(false));

    // Real FRIDAY runtime — the event bus every subsystem publishes to.
    let friday = Friday::new();
    let bus = friday.bus().clone();

    let explicit_listen: Option<SocketAddr> = std::env::var("FRIDAY_LISTEN")
        .ok()
        .and_then(|s| s.parse().ok());
    let listen: SocketAddr = explicit_listen.unwrap_or_else(|| DEFAULT_LISTEN.parse().unwrap());
    if !listen.ip().is_loopback() {
        tracing::warn!(
            "listen {listen} is not loopback; the shell contract expects loopback as the trust boundary"
        );
    }
    // Bind up front so the real address can be published in state.json. The
    // default port may belong to another user's daemon on the same machine;
    // then take any free loopback port (clients read `listen` from
    // state.json). An explicitly configured FRIDAY_LISTEN must be honoured or
    // fail.
    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(l) => Ok(l),
        Err(e) if explicit_listen.is_none() && e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::warn!(
                "{listen} is in use (another user's FRIDAY?); using a free loopback port"
            );
            tokio::net::TcpListener::bind("127.0.0.1:0").await
        }
        Err(e) => Err(e),
    };

    // Shell ingress: loopback WebSocket, NO QR-pair auth (same host; the
    // loopback bind + per-run session token are the trust boundary).
    let server = WsServer::new(listen).with_require_auth(false);
    tracing::info!("shell command ingress on {listen} (loopback, token-gated)");
    let outbound: broadcast::Sender<_> = server.broadcast_tx();
    let sessions = server.sessions();
    let mut ingress = server.subscribe_commands();

    // The file-backed shell-state contract.
    let state_path = if let Ok(p) = std::env::var("FRIDAY_STATE_FILE") {
        PathBuf::from(p)
    } else {
        ShellState::default_path()
    };
    tracing::info!("shell state at {}", state_path.display());

    let token = generate_token();
    let state = Arc::new(Mutex::new(ShellState::starting(token.clone())));
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind the command ingress on {listen}: {e}");
            let mut st = state.lock().await;
            st.status = "failed".into();
            write_state(&state_path, &render(&st));
            std::process::exit(1);
        }
    };
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| listen.to_string());
    state.lock().await.listen = bound.clone();
    tracing::info!("shell command ingress bound on {bound}");

    // Seed initial `starting` document so the shell sees something on first read.
    write_state(&state_path, &render(&*state.lock().await));

    // Real host telemetry onto the bus.
    let probe = SystemProbe::new(bus.clone());
    tokio::spawn(async move { probe.run().await });

    // Model-backed investigation + heuristic fallback. Bounded request avoids
    // a hung-local-model stall (same fabric tuning as the mobile daemon).
    let mut fabric = InferenceFabric::default_from_env();
    fabric.request_timeout = Some(Duration::from_secs(15));
    // OpenCode Zen (the keyless default) rotates its free models: swap the
    // static fallback for the live free lineup. Bounded so a slow or offline
    // network never delays startup.
    match tokio::time::timeout(Duration::from_secs(8), fabric.refresh_opencode_catalog()).await {
        Ok(true) => tracing::info!("[providers] OpenCode Zen: live free models loaded"),
        _ => tracing::warn!(
            "[providers] OpenCode Zen catalog unavailable; using built-in free models"
        ),
    }
    let fabric = Arc::new(fabric);
    tracing::info!(
        "[Investigation] fabric: {} providers / {} models registered",
        fabric.providers.len(),
        fabric.models.len()
    );

    // Persistent memory: one durable journal backs the Executive's admission
    // gate + memory tools. Episodes survive restarts.
    let memory_dir =
        std::env::var("FRIDAY_MEMORY_DIR").unwrap_or_else(|_| "friday-data".to_string());
    let journal = MemoryJournal::open(std::path::Path::new(&memory_dir).join("episodes.jsonl"))
        .expect("open memory journal");
    tracing::info!(
        "persistent memory journal at {}/episodes.jsonl: {} episode(s) replayed",
        memory_dir,
        journal.len()
    );

    // Voice: configuration, never a requirement — a spoken turn becomes a
    // `plan.goal`, exactly like a typed goal.
    let voice = VoiceConfig::from_env();
    {
        let mut st = state.lock().await;
        st.voice = match &voice {
            VoiceConfig::Enabled(_) => "on".into(),
            VoiceConfig::Disabled => "off".into(),
            VoiceConfig::Misconfigured(_) => "misconfigured".into(),
        };
    }
    if let VoiceConfig::Enabled(cfg) = &voice {
        tracing::info!(
            "[voice] LIVE ON: real mic -> whisper (exe={}, model={})",
            cfg.executable.display(),
            cfg.model.display()
        );
    }
    if let Some(mic) = voice.sensor() {
        tracing::info!(
            "[voice] intake active: spoken turns enter plan.goal (capture: {}, endpoint: {})",
            mic.capture_available(),
            mic.capture_device_id()
        );
        let intake = VoiceIntake::new(bus.clone(), mic);
        tokio::spawn(intake.run(Duration::from_secs(1)));
    }
    write_state(&state_path, &render(&*state.lock().await));

    // R5-2-5-2: the explicit Screen context read.
    //   ContextCollector (real OS) → ContextGate → EgressGate → FabricSeam.
    // The screenshot goes to a vision model routed like any ask (the user's
    // provider before keyless OpenCode Zen), unless FRIDAY_CONTEXT_PROVIDER
    // names one provider to always use.
    let context_provider = std::env::var("FRIDAY_CONTEXT_PROVIDER")
        .ok()
        .filter(|p| !p.trim().is_empty());
    let context_label = context_provider.as_deref().unwrap_or("routed");
    let screen_reader = Arc::new(friday_mobile::context_read::ScreenReader::new(
        ContextCollector::new(RealCommandRunner::default()),
        friday_mobile::context_read::FabricProviderSeam::new(
            fabric.clone(),
            context_provider.as_deref(),
        ),
        context_label,
    ));
    tracing::info!(
        "[context.read] Screen reader armed (collector=Real, provider={context_label}); no ambient capture"
    );

    // The assistant loop. Consumes `plan.goal` and runs each goal through the
    // real Executive with real evidence adapters.
    GoalRunner::with_fabric_and_memory(bus.clone(), fabric, journal, voice).start();

    // State writer: keep `state.json` in step with bus lifecycle events and a
    // heartbeat so the shell always has fresh truth even when idle.
    let mut state_bus = bus.subscribe();
    let state_writer_path = state_path.clone();
    let state_writer_lock = state.clone();
    {
        let stopping = stopping.clone();
        tokio::spawn(async move {
            loop {
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let heartbeat = Duration::from_secs(30);
                let ev = tokio::time::timeout(heartbeat, state_bus.recv()).await;
                match ev {
                    Ok(Ok(ev)) => {
                        let mut st = state_writer_lock.lock().await;
                        let next = friday_mobile::shell_state::apply_event(st.clone(), &ev);
                        *st = next;
                        drop(st);
                        write_state(
                            &state_writer_path,
                            &render(&*state_writer_lock.lock().await),
                        );
                    }
                    Ok(Err(_)) => break, // bus closed
                    Err(_) => {
                        // Heartbeat: just re-stamp.
                        let st = state_writer_lock.lock().await;
                        write_state(&state_writer_path, &render(&st));
                    }
                }
            }
        });
    }

    // Translate shell commands → real bus events, and echo the accepted one
    // into state.json so the UI can show it. This is the "one controlled
    // command" surface.
    let gateway = CommandGateway::new(bus.clone(), sessions.clone());
    let result_out = outbound.clone();
    let state_cmd_lock = state.clone();
    let state_cmd_path = state_path.clone();
    let context_reader = screen_reader.clone();
    tokio::spawn(async move {
        while let Ok(cmd) = ingress.recv().await {
            // Token gate: reject commands that don't present the current token.
            let presented = cmd
                .params
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if presented.is_empty() || presented != state_cmd_lock.lock().await.token.as_str() {
                let _ = result_out.send(friday_mobile::protocol::ServerMessage::CommandResult {
                    id: cmd.id.clone(),
                    ok: false,
                    result: serde_json::json!({ "error": "forbidden: missing or invalid token" }),
                });
                continue;
            }
            // R5-2-5-2: explicit Screen context read. Scoped to `Screen` only;
            // never ambient. Consent is a per-use, request-attributable opt-in.
            if cmd.method == friday_mobile::method::CONTEXT_READ {
                let want_consent = cmd
                    .params
                    .get("consent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if want_consent {
                    // The user explicitly opted in for this request. Without it,
                    // `read()` is blocked at the ContextGate and the provider
                    // receives nothing.
                    context_reader.consent();
                }
                // Optional: what the user wants to know about the screen.
                let question = cmd
                    .params
                    .get("question")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let (ok, result) = match context_reader.ask(question.as_deref()).await {
                    Ok(response) => {
                        tracing::info!("[context.read] screen delivered to provider");
                        (true, serde_json::json!({ "response": response }))
                    }
                    Err(e) => {
                        tracing::warn!("[context.read] {e}");
                        (false, serde_json::json!({ "error": e.to_string() }))
                    }
                };
                let mut st = state_cmd_lock.lock().await;
                st.context_read = Some(friday_mobile::shell_state::ContextReadRecord {
                    method: cmd.method.clone(),
                    ms: now_ms(),
                    ok,
                    channel: "screen".to_string(),
                });
                write_state(&state_cmd_path, &render(&st));
                let _ = result_out.send(friday_mobile::protocol::ServerMessage::CommandResult {
                    id: cmd.id.clone(),
                    ok,
                    result,
                });
                continue;
            }
            let result = gateway.dispatch(&cmd).await;
            if cmd.method == friday_mobile::method::PLANNER_GOAL {
                let goal = cmd
                    .params
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let mut st = state_cmd_lock.lock().await;
                st.command = Some(CommandRecord {
                    method: cmd.method.clone(),
                    accepted_ms: now_ms(),
                    goal,
                });
                write_state(&state_cmd_path, &render(&st));
            }
            if result_out.send(result).is_err() {
                break;
            }
        }
    });

    // Signal handling: SIGTERM/SIGINT → graceful stop. The ws loop below
    // selects on `stopping`.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let flag = stopping.clone();
        tokio::spawn(async move {
            let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
            let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
            tokio::select! {
                _ = term.recv() => { flag.store(true, Ordering::Relaxed); }
                _ = int.recv() => { flag.store(true, Ordering::Relaxed); }
            }
        });
    }
    #[cfg(not(unix))]
    {
        let flag = stopping.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            flag.store(true, Ordering::Relaxed);
        });
    }

    // systemd readiness (no-op without NOTIFY_SOCKET).
    sd_notify("READY=1\nSTATUS=FRIDAY running\n");

    // Mark state `running` and publish once for the shell's first read.
    {
        let mut st = state.lock().await;
        st.status = "running".into();
        write_state(&state_path, &render(&st));
    }

    tracing::info!("FRIDAY linux daemon listening on {listen}");

    // Run the WS server until told to stop.
    let failed = tokio::select! {
        r = server.run_on(listener) => {
            match r {
                Ok(()) => {
                    tracing::info!("shell ingress closed cleanly");
                    false
                }
                Err(e) => {
                    // e.g. another daemon (another user's session) already owns
                    // the port. Treat it as a failure, not a clean stop.
                    tracing::error!("shell ingress stopped: {e}");
                    true
                }
            }
        }
        _ = wait_for_stop(&stopping) => {
            tracing::info!("shutdown requested; stopping cleanly");
            let mut st = state.lock().await;
            st.status = "stopping".into();
            write_state(&state_path, &render(&st));
            false
        }
    };

    // Never leave state.json claiming "running" after exit: the bar, doctor
    // and friday-ctl all trust it.
    {
        let mut st = state.lock().await;
        st.status = if failed { "failed" } else { "stopped" }.into();
        write_state(&state_path, &render(&st));
    }
    sd_notify("STOPPING=1\nSTATUS=FRIDAY stopped\n");
    tracing::info!("FRIDAY linux daemon stopped");
    if failed {
        // Non-zero so systemd marks the unit failed and Restart=on-failure
        // retries it.
        std::process::exit(1);
    }
}

async fn wait_for_stop(flag: &Arc<AtomicBool>) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
