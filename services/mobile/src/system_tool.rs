//! System packages on FRIDAY OS: update the system, install or remove a
//! package, through the root helper (`org.friday.Helper1`).
//!
//! The model only turns the request into one of three typed operations. Each
//! one is destructive, so it always stops and asks first with the exact
//! action; nothing runs on the request alone. Once confirmed, only that
//! operation runs, with no model in between: the helper asks the person again
//! (polkit), checks the caller, the package name and the repos, and runs a
//! fixed pacman command. The result is read back from the helper's job.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use friday_goal_execution::{Action, Capability, ExecutionContext, Tool, ToolResult};
use friday_investigation::LlmStrategyClient;
use serde_json::{json, Value};

use crate::goal_runner::ConfirmationGate;

/// The capability the planner routes package goals to.
pub const CAPABILITY: &str = "system.packages";

const MODEL_TIMEOUT: Duration = Duration::from_secs(45);
/// Longest FRIDAY waits for a job (a full update can be slow).
const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
const LOG_LINES: usize = 12;

/// Whether this machine can have the helper: Linux, and the owner hasn't
/// turned system work off (`FRIDAY_SYSTEM=0`).
pub fn enabled() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("FRIDAY_SYSTEM")
            .map(|v| v != "0")
            .unwrap_or(true)
}

/// One thing the helper can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysOp {
    Update,
    Install(String),
    Remove(String),
}

/// A package name pacman would take: lowercase letters, digits and `@ . _ + -`,
/// not starting with `-`. The helper checks again (and that it is in the repos).
pub fn valid_package(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@._+-".contains(c))
}

impl SysOp {
    /// The exact action the person confirms; the same text runs afterwards.
    pub fn exact(&self) -> String {
        match self {
            SysOp::Update => "system update (pacman -Syu)".to_string(),
            SysOp::Install(p) => format!("system install {p} (pacman -S --needed {p})"),
            SysOp::Remove(p) => format!("system remove {p} (pacman -R {p})"),
        }
    }

    /// Reads a confirmed action back; `None` for anything this tool did not word.
    pub fn parse(confirmed: &str) -> Option<SysOp> {
        let op = if confirmed == SysOp::Update.exact() {
            SysOp::Update
        } else {
            let rest = confirmed.strip_prefix("system ")?;
            let (verb, rest) = rest.split_once(' ')?;
            let package = rest.split(' ').next()?;
            let op = match verb {
                "install" => SysOp::Install(package.to_string()),
                "remove" => SysOp::Remove(package.to_string()),
                _ => return None,
            };
            if !valid_package(package) || op.exact() != confirmed {
                return None;
            }
            op
        };
        Some(op)
    }

    fn verb(&self) -> &'static str {
        match self {
            SysOp::Update => "update",
            SysOp::Install(_) => "install",
            SysOp::Remove(_) => "remove",
        }
    }

    fn package(&self) -> Option<&str> {
        match self {
            SysOp::Update => None,
            SysOp::Install(p) | SysOp::Remove(p) => Some(p),
        }
    }

    /// A plain-words preview of what this operation will change.
    pub fn preview(&self) -> String {
        sysop_preview(self)
    }
}

/// Check if a package is installed using `pacman -Q <pkg>`.
fn check_pacman_installed(pkg: &str) -> Option<bool> {
    std::process::Command::new("pacman")
        .args(["-Q", pkg])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|s| s.success())
}

/// Builds preview text for a system operation from package presence.
pub fn sysop_preview_text(op: &SysOp, installed: Option<bool>) -> String {
    match op {
        SysOp::Update => "Updates every package on this computer.".to_string(),
        SysOp::Remove(p) => format!("Removes {p}."),
        SysOp::Install(p) => match installed {
            Some(true) => format!("Reinstalls {p} (already installed)."),
            Some(false) => format!("Installs {p} (not installed now)."),
            None => format!("Installs {p}."),
        },
    }
}

pub fn sysop_preview(op: &SysOp) -> String {
    let installed = match op {
        SysOp::Install(p) => check_pacman_installed(p),
        _ => None,
    };
    sysop_preview_text(op, installed)
}

/// Formats the completion verdict for a finished system package job.
pub fn package_job_verdict(op: &SysOp, installed: Option<bool>) -> String {
    match op {
        SysOp::Install(p) => match installed {
            Some(false) => format!("Partly: the helper finished but {p} is not installed."),
            _ => format!("Done: {p} is installed."),
        },
        SysOp::Remove(p) => match installed {
            Some(true) => format!("Partly: the helper finished but {p} is still installed."),
            _ => format!("Done: {p} is gone."),
        },
        SysOp::Update => format!("Done: {}", op.exact()),
    }
}

/// The root helper, as seen from the daemon.
#[async_trait]
pub trait HelperLink: Send + Sync {
    /// Whether the helper is there to call (installed and turned on).
    async fn present(&self) -> bool;
    /// Starts the operation; the helper asks the person first. Returns a job id.
    async fn start(&self, op: &SysOp) -> Result<String, String>;
    /// The job's state (`queued`, `running`, `done`, `failed`) and the end of its log.
    async fn job(&self, id: &str) -> Result<(String, String), String>;
}

/// The helper on the system bus.
#[cfg(target_os = "linux")]
pub struct DbusHelper;

#[cfg(target_os = "linux")]
const BUS_NAME: &str = "org.friday.Helper1";

#[cfg(target_os = "linux")]
#[async_trait]
impl HelperLink for DbusHelper {
    async fn present(&self) -> bool {
        let Ok(conn) = zbus::Connection::system().await else {
            return false;
        };
        let Ok(bus) = zbus::fdo::DBusProxy::new(&conn).await else {
            return false;
        };
        let Ok(name) = zbus::names::BusName::try_from(BUS_NAME) else {
            return false;
        };
        if bus.name_has_owner(name).await.unwrap_or(false) {
            return true;
        }
        bus.list_activatable_names()
            .await
            .map(|names| names.iter().any(|n| n.as_str() == BUS_NAME))
            .unwrap_or(false)
    }

    async fn start(&self, op: &SysOp) -> Result<String, String> {
        let conn = zbus::Connection::system()
            .await
            .map_err(|e| format!("couldn't reach the system bus: {e}"))?;
        let path = "/org/friday/Helper1";
        let reply = match op {
            SysOp::Update => {
                conn.call_method(Some(BUS_NAME), path, Some(BUS_NAME), "UpdateSystem", &())
                    .await
            }
            SysOp::Install(p) => {
                conn.call_method(
                    Some(BUS_NAME),
                    path,
                    Some(BUS_NAME),
                    "InstallPackage",
                    &(p,),
                )
                .await
            }
            SysOp::Remove(p) => {
                conn.call_method(Some(BUS_NAME), path, Some(BUS_NAME), "RemovePackage", &(p,))
                    .await
            }
        }
        .map_err(|e| plain_dbus_error(&e.to_string()))?;
        reply
            .body()
            .deserialize::<String>()
            .map_err(|e| format!("unexpected reply from the helper: {e}"))
    }

    async fn job(&self, id: &str) -> Result<(String, String), String> {
        let conn = zbus::Connection::system()
            .await
            .map_err(|e| format!("couldn't reach the system bus: {e}"))?;
        let reply = conn
            .call_method(
                Some(BUS_NAME),
                "/org/friday/Helper1",
                Some(BUS_NAME),
                "JobStatus",
                &(id,),
            )
            .await
            .map_err(|e| plain_dbus_error(&e.to_string()))?;
        reply
            .body()
            .deserialize::<(String, String)>()
            .map_err(|e| format!("unexpected reply from the helper: {e}"))
    }
}

/// What the person is told when a call to the helper fails.
#[cfg(any(target_os = "linux", test))]
fn plain_dbus_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("not authorized") || lower.contains("denied") || lower.contains("dismissed") {
        "The system asked for permission and it was not given, so nothing was changed.".into()
    } else if lower.contains("serviceunknown") || lower.contains("not provided") {
        "The system helper is off. Turn it on with: friday helper enable".into()
    } else {
        // The helper's own refusal (not in the repos, protected package, a job already running).
        raw.rsplit(": ").next().unwrap_or(raw).trim().to_string()
    }
}

pub struct SystemTool {
    model: Arc<dyn LlmStrategyClient>,
    link: Arc<dyn HelperLink>,
    gate: Arc<Mutex<ConfirmationGate>>,
    abort: Arc<AtomicBool>,
    audit_dir: Option<PathBuf>,
    poll: Duration,
}

const GUIDE: &str = r#"Turn the person's request into one operation on their Arch Linux system.
Reply with ONLY one JSON object, nothing else:
  {"op": "update"}                         update the whole system
  {"op": "install", "package": "<name>"}   install one package from the Arch repositories
  {"op": "remove", "package": "<name>"}    remove one package
  {"op": "none", "say": "<why not>"}       the request is not one of these
Package names are the exact lowercase Arch package name (for example "htop", "vlc", "firefox")."#;

fn first_json(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

impl SystemTool {
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn new(
        model: Arc<dyn LlmStrategyClient>,
        link: Arc<dyn HelperLink>,
        gate: Arc<Mutex<ConfirmationGate>>,
        abort: Arc<AtomicBool>,
        audit_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            model,
            link,
            gate,
            abort,
            audit_dir,
            poll: Duration::from_secs(2),
        }
    }

    /// One line in `actions.jsonl`: what was done to the system, and how it went.
    fn record(&self, op: &SysOp, ok: bool, goal: &str) {
        let Some(dir) = &self.audit_dir else { return };
        crate::desktop_tool::append_audit(
            dir,
            &json!({
                "ts": chrono::Utc::now().to_rfc3339(),
                "verb": format!("system_{}", op.verb()),
                "params": op.package().map(|p| json!({"package": p})).unwrap_or(json!({})),
                "risk": "destructive",
                "ok": ok,
                "goal": goal.chars().take(120).collect::<String>(),
            }),
        );
    }

    async fn run(&self, op: &SysOp, goal: &str) -> ToolResult {
        if !self.link.present().await {
            return ToolResult::error(
                "The system helper is off, so FRIDAY can't change packages. Turn it on with: friday helper enable",
            );
        }
        let job = match self.link.start(op).await {
            Ok(id) => id,
            Err(e) => {
                self.record(op, false, goal);
                return ToolResult::error(e);
            }
        };
        let started = std::time::Instant::now();
        loop {
            if self.abort.load(Ordering::SeqCst) {
                return ToolResult::error(
                    "Stopped waiting. The job keeps running on the system; `friday actions` shows what was started.",
                );
            }
            match self.link.job(&job).await {
                Ok((state, log)) if state == "done" => {
                    self.record(op, true, goal);
                    if !crate::features::on("action-check") {
                        return ToolResult::success(format!(
                            "Done: {}\n{}",
                            op.exact(),
                            tail(&log)
                        ));
                    }
                    let installed = op.package().and_then(check_pacman_installed);
                    let prefix = package_job_verdict(op, installed);
                    return ToolResult::success(format!("{prefix}\n{}", tail(&log)));
                }
                Ok((state, log)) if state == "failed" => {
                    self.record(op, false, goal);
                    return ToolResult::error(format!("{} failed.\n{}", op.exact(), tail(&log)));
                }
                Ok(_) => {}
                Err(e) => {
                    self.record(op, false, goal);
                    return ToolResult::error(e);
                }
            }
            if started.elapsed() > MAX_WAIT {
                return ToolResult::error(
                    "Still running after 30 minutes; FRIDAY stopped waiting. The job continues on the system.",
                );
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let from = lines.len().saturating_sub(LOG_LINES);
    lines[from..].join("\n")
}

#[async_trait]
impl Tool for SystemTool {
    fn name(&self) -> &str {
        "system"
    }

    fn description(&self) -> &str {
        "Updates the system and installs or removes packages through the root helper (system.packages)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Custom(CAPABILITY.into())]
    }

    async fn execute(&self, action: &Action, _ctx: &ExecutionContext) -> ToolResult {
        // A confirmed operation: run exactly that and nothing else.
        let confirmed = self.gate.lock().unwrap().confirm.clone();
        if let Some(c) = confirmed.filter(|c| c.trim_start().starts_with("system ")) {
            if let Err(err) = self.gate.lock().unwrap().check_or_record("system", &c) {
                return err;
            }
            return match SysOp::parse(c.trim()) {
                Some(op) => self.run(&op, "(confirmed)").await,
                None => ToolResult::error("that confirmation is not for a system action"),
            };
        }

        if self.abort.load(Ordering::SeqCst) {
            return ToolResult::error("Stopped. Nothing was changed.");
        }
        let goal = friday_goal_execution::planning::goal_segment(&action.instruction)
            .trim()
            .to_string();
        let prompt = format!("{GUIDE}\n\nThe person's request: {goal}\n\nYour reply (JSON only):");
        let reply = match tokio::time::timeout(MODEL_TIMEOUT, self.model.complete(&prompt)).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                tracing::warn!("couldn't ask the model: {e}");
                return ToolResult::error(crate::failures::plain_model_error(&e.to_string()));
            }
            Err(_) => {
                return ToolResult::error("the model took too long to understand the request")
            }
        };
        let Some(choice) = first_json(&reply) else {
            return ToolResult::error("couldn't tell which package action was meant");
        };
        let package = choice.get("package").and_then(Value::as_str).unwrap_or("");
        let op = match choice.get("op").and_then(Value::as_str) {
            Some("update") => SysOp::Update,
            Some("install") if valid_package(package) => SysOp::Install(package.to_string()),
            Some("remove") if valid_package(package) => SysOp::Remove(package.to_string()),
            Some("install" | "remove") => {
                return ToolResult::error("that is not a package name I can use")
            }
            _ => {
                let say = choice.get("say").and_then(Value::as_str).unwrap_or("");
                return ToolResult::error(if say.is_empty() {
                    "That is not something I can do to the system's packages.".to_string()
                } else {
                    say.chars().take(300).collect()
                });
            }
        };
        // Always ask first, naming the exact action; nothing has been changed.
        let preview = if crate::features::on("action-check") {
            Some(op.preview())
        } else {
            None
        };
        if let Err(err) =
            self.gate
                .lock()
                .unwrap()
                .check_or_record_with("system", &op.exact(), preview)
        {
            return err;
        }
        self.run(&op, &goal).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Model(Mutex<VecDeque<String>>, Mutex<usize>);

    impl Model {
        fn new(reply: &str) -> Arc<Self> {
            Arc::new(Model(
                Mutex::new(VecDeque::from([reply.to_string()])),
                Mutex::new(0),
            ))
        }
        fn asked(&self) -> usize {
            *self.1.lock().unwrap()
        }
    }

    #[async_trait]
    impl LlmStrategyClient for Model {
        async fn complete(&self, _: &str) -> Result<String, String> {
            *self.1.lock().unwrap() += 1;
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "none left".into())
        }
    }

    #[derive(Default)]
    struct Link {
        absent: bool,
        started: Mutex<Vec<SysOp>>,
        states: Mutex<VecDeque<(String, String)>>,
        start_error: Option<String>,
    }

    impl Link {
        fn finishing(state: &str, log: &str) -> Arc<Self> {
            let l = Link::default();
            l.states
                .lock()
                .unwrap()
                .push_back(("running".into(), String::new()));
            l.states
                .lock()
                .unwrap()
                .push_back((state.into(), log.into()));
            Arc::new(l)
        }
        fn started(&self) -> Vec<SysOp> {
            self.started.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HelperLink for Link {
        async fn present(&self) -> bool {
            !self.absent
        }
        async fn start(&self, op: &SysOp) -> Result<String, String> {
            if let Some(e) = &self.start_error {
                return Err(e.clone());
            }
            self.started.lock().unwrap().push(op.clone());
            Ok("job-1".into())
        }
        async fn job(&self, _: &str) -> Result<(String, String), String> {
            Ok(self
                .states
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(("running".into(), String::new())))
        }
    }

    fn tool(model: Arc<Model>, link: Arc<Link>, gate: Arc<Mutex<ConfirmationGate>>) -> SystemTool {
        let mut t = SystemTool::new(model, link, gate, Arc::default(), None);
        t.poll = Duration::from_millis(1);
        t
    }

    async fn run(t: &SystemTool, goal: &str) -> ToolResult {
        let action = Action::single(goal, Capability::Custom(CAPABILITY.into()), "g1");
        t.execute(&action, &ExecutionContext::for_goal("g1")).await
    }

    fn gate() -> Arc<Mutex<ConfirmationGate>> {
        Arc::new(Mutex::new(ConfirmationGate::default()))
    }

    #[test]
    fn the_confirmed_text_round_trips_and_nothing_else_parses() {
        for op in [
            SysOp::Update,
            SysOp::Install("htop".into()),
            SysOp::Remove("python-pip".into()),
        ] {
            assert_eq!(SysOp::parse(&op.exact()), Some(op));
        }
        for bad in [
            "system install htop; rm -rf / (pacman -S --needed htop; rm -rf /)",
            "system install -Syu (pacman -S --needed -Syu)",
            "system install htop (pacman -S --needed vlc)",
            "system reboot",
            "desktop close {}",
            "system install ../x (pacman -S --needed ../x)",
        ] {
            assert_eq!(SysOp::parse(bad), None, "{bad}");
        }
        assert!(valid_package("lib32-glibc") && valid_package("gtk4"));
        assert!(!valid_package("-x") && !valid_package("Htop") && !valid_package("a b"));
    }

    #[tokio::test]
    async fn a_request_asks_first_and_changes_nothing() {
        let g = gate();
        let link = Link::finishing("done", "");
        let t = tool(
            Model::new(r#"{"op":"install","package":"htop"}"#),
            link.clone(),
            g.clone(),
        );
        let res = run(&t, "install htop").await;
        assert!(!res.success);
        assert!(res
            .error
            .unwrap()
            .contains("system install htop (pacman -S --needed htop)"));
        assert!(link.started().is_empty());
        assert_eq!(g.lock().unwrap().pending.as_ref().unwrap().tool, "system");
    }

    #[tokio::test]
    async fn once_confirmed_only_that_runs_and_the_model_is_not_asked() {
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Install("htop".into()).exact());
        let model = Model::new("unused");
        let link = Link::finishing("done", "installing htop\nok");
        let t = tool(model.clone(), link.clone(), g);
        let res = run(&t, "install htop").await;
        assert!(res.success, "{:?}", res.error);
        assert_eq!(link.started(), vec![SysOp::Install("htop".into())]);
        assert_eq!(model.asked(), 0);
        assert!(res.output.contains("installing htop"));
    }

    #[tokio::test]
    async fn a_confirmation_runs_what_was_confirmed_not_a_new_request() {
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Remove("vlc".into()).exact());
        let link = Link::finishing("done", "");
        let t = tool(
            Model::new(r#"{"op":"install","package":"htop"}"#),
            link.clone(),
            g,
        );
        // The confirmed text is for removing vlc: it runs that, not the request.
        let res = run(&t, "install htop").await;
        assert!(res.success);
        assert_eq!(link.started(), vec![SysOp::Remove("vlc".into())]);
    }

    #[tokio::test]
    async fn a_desktop_confirmation_is_not_ours() {
        let g = gate();
        g.lock().unwrap().confirm = Some("desktop close {}".into());
        let link = Link::finishing("done", "");
        let t = tool(Model::new(r#"{"op":"update"}"#), link.clone(), g);
        let res = run(&t, "update my system").await;
        assert!(!res.success);
        assert!(link.started().is_empty());
    }

    #[tokio::test]
    async fn an_off_helper_says_how_to_turn_it_on() {
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Update.exact());
        let link = Arc::new(Link {
            absent: true,
            ..Default::default()
        });
        let t = tool(Model::new("unused"), link.clone(), g);
        let res = run(&t, "update my system").await;
        assert!(res.error.unwrap().contains("friday helper enable"));
        assert!(link.started().is_empty());
    }

    #[tokio::test]
    async fn a_failed_job_reports_the_end_of_its_log() {
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Update.exact());
        let log = (1..=30)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let link = Link::finishing("failed", &log);
        let t = tool(Model::new("unused"), link, g);
        let err = run(&t, "update").await.error.unwrap();
        assert!(
            err.contains("line 30") && !err.contains("line 1\n"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn the_helpers_refusal_and_a_denied_prompt_are_plain() {
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Install("nope".into()).exact());
        let link = Arc::new(Link {
            start_error: Some("package \"nope\" is not in the repositories".into()),
            ..Default::default()
        });
        let t = tool(Model::new("unused"), link, g);
        assert!(run(&t, "x")
            .await
            .error
            .unwrap()
            .contains("not in the repositories"));
        assert!(
            plain_dbus_error("org.freedesktop.DBus.Error.AccessDenied: not authorized")
                .contains("nothing was changed")
        );
        assert!(
            plain_dbus_error("org.freedesktop.DBus.Error.ServiceUnknown: x")
                .contains("friday helper enable")
        );
    }

    #[tokio::test]
    async fn bad_model_replies_change_nothing() {
        for reply in [
            "no json here",
            r#"{"op":"install","package":"htop; reboot"}"#,
            r#"{"op":"install"}"#,
            r#"{"op":"reboot"}"#,
            r#"{"op":"none","say":"I can't do that"}"#,
        ] {
            let link = Link::finishing("done", "");
            let t = tool(Model::new(reply), link.clone(), gate());
            let res = run(&t, "do it").await;
            assert!(!res.success, "{reply}");
            assert!(link.started().is_empty(), "{reply}");
        }
    }

    #[tokio::test]
    async fn a_stop_ends_the_wait_and_every_run_is_audited() {
        let dir = std::env::temp_dir().join(format!("friday-sys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let g = gate();
        g.lock().unwrap().confirm = Some(SysOp::Install("htop".into()).exact());
        let link = Link::finishing("done", "ok");
        let mut t = SystemTool::new(
            Model::new("unused"),
            link,
            g,
            Arc::default(),
            Some(dir.clone()),
        );
        t.poll = Duration::from_millis(1);
        assert!(run(&t, "x").await.success);
        let log = std::fs::read_to_string(dir.join("actions.jsonl")).unwrap();
        assert!(
            log.contains("system_install") && log.contains("htop") && log.contains("destructive")
        );

        t.abort.store(true, Ordering::SeqCst);
        t.gate.lock().unwrap().confirm = Some(SysOp::Update.exact());
        let res = run(&t, "x").await;
        assert!(res.error.unwrap().contains("Stopped waiting"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Against the real system bus, with `friday helper enable` done: the
    /// helper is there, and it refuses this test program as a caller (only
    /// the FRIDAY daemon may call), in words a person can read.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "needs the root helper enabled on the system bus (the VM)"]
    async fn real_helper_is_present_and_refuses_an_unknown_caller() {
        let link = DbusHelper;
        assert!(link.present().await, "run `friday helper enable` first");
        let err = link.start(&SysOp::Update).await.unwrap_err();
        println!("refusal: {err}");
        assert!(!err.is_empty() && !err.contains("job-"));
    }

    #[test]
    fn sysop_preview_decisions() {
        assert_eq!(
            sysop_preview_text(&SysOp::Update, None),
            "Updates every package on this computer."
        );
        assert_eq!(
            sysop_preview_text(&SysOp::Remove("firefox".into()), None),
            "Removes firefox."
        );
        assert_eq!(
            sysop_preview_text(&SysOp::Install("htop".into()), Some(false)),
            "Installs htop (not installed now)."
        );
        assert_eq!(
            sysop_preview_text(&SysOp::Install("htop".into()), Some(true)),
            "Reinstalls htop (already installed)."
        );
        assert_eq!(
            sysop_preview_text(&SysOp::Install("htop".into()), None),
            "Installs htop."
        );
    }

    #[test]
    fn package_job_verdict_decisions() {
        // Install
        assert_eq!(
            package_job_verdict(&SysOp::Install("htop".into()), Some(true)),
            "Done: htop is installed."
        );
        assert_eq!(
            package_job_verdict(&SysOp::Install("htop".into()), Some(false)),
            "Partly: the helper finished but htop is not installed."
        );
        assert_eq!(
            package_job_verdict(&SysOp::Install("htop".into()), None),
            "Done: htop is installed."
        );

        // Remove
        assert_eq!(
            package_job_verdict(&SysOp::Remove("htop".into()), Some(false)),
            "Done: htop is gone."
        );
        assert_eq!(
            package_job_verdict(&SysOp::Remove("htop".into()), Some(true)),
            "Partly: the helper finished but htop is still installed."
        );
        assert_eq!(
            package_job_verdict(&SysOp::Remove("htop".into()), None),
            "Done: htop is gone."
        );

        // Update
        assert_eq!(
            package_job_verdict(&SysOp::Update, None),
            "Done: system update (pacman -Syu)"
        );
    }
}
