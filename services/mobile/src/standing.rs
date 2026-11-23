//! Standing tasks: "every morning at 8, tell me what's on my calendar",
//! "when the battery drops below 20%, tell me".
//!
//! A task is a goal plus a trigger, kept in `standing.json` beside
//! `state.json` (owner-readable only). The daemon checks the file every
//! [`TICK`], and when a task is due it runs the goal the way the person's own
//! ask runs: through `friday ask`, so it gets the same consent classes,
//! secret masking, egress log, notification and answer history. A destructive
//! step still stops and asks; nobody has said yes to it in advance.
//!
//! `friday tasks` lists, pauses, resumes and deletes by editing the same file.
//!
//! On Windows there is no `friday` CLI: the tray is what asks, so a due goal
//! goes into `standing-due.jsonl` and the tray runs it as an ask of its own
//! (its Tasks page edits `standing.json`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Datelike, Duration as Span, Local, NaiveDateTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

/// How often the daemon looks for a due task.
pub const TICK: Duration = Duration::from_secs(20);
/// Most tasks one person can have.
pub const MAX_TASKS: usize = 20;
/// Longest goal, in characters.
pub const MAX_GOAL: usize = 300;
/// Shortest repeat interval, in minutes.
pub const MIN_EVERY_MINUTES: u32 = 5;
/// A daily task missed while the computer was off still runs if it is at most
/// this late; later than that it waits for tomorrow.
const CATCH_UP: Span = Span::hours(6);

/// When a task runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// Every day (or weekday) at a local time.
    Daily {
        hour: u8,
        minute: u8,
        #[serde(default)]
        weekdays_only: bool,
    },
    /// Every so many minutes, counted from the last run.
    Every { minutes: u32 },
    /// Once each time the battery falls under `percent` while unplugged.
    BatteryBelow { percent: u8 },
    /// Once, at `at` (unix seconds): a reminder. Removed after it runs.
    Once { at: i64 },
}

impl Trigger {
    /// A short phrase for lists: "every day at 08:00".
    pub fn describe(&self) -> String {
        match self {
            Trigger::Daily {
                hour,
                minute,
                weekdays_only,
            } => format!(
                "every {} at {hour:02}:{minute:02}",
                if *weekdays_only { "weekday" } else { "day" }
            ),
            Trigger::Every { minutes } if minutes % 60 == 0 => match minutes / 60 {
                1 => "every hour".to_string(),
                h => format!("every {h} hours"),
            },
            Trigger::Every { minutes } => format!("every {minutes} minutes"),
            Trigger::BatteryBelow { percent } => format!("when the battery is under {percent}%"),
            Trigger::Once { at } => {
                format!("once at {}", local_naive(*at).format("%H:%M on %-d %b"))
            }
        }
    }

    fn valid(&self) -> Result<(), String> {
        match self {
            Trigger::Daily { hour, minute, .. } if *hour > 23 || *minute > 59 => {
                Err("that is not a time of day".into())
            }
            Trigger::Every { minutes } if *minutes < MIN_EVERY_MINUTES => Err(format!(
                "the shortest repeat is every {MIN_EVERY_MINUTES} minutes"
            )),
            Trigger::Every { minutes } if *minutes > 7 * 24 * 60 => {
                Err("the longest repeat is once a week".into())
            }
            Trigger::BatteryBelow { percent } if !(1..=99).contains(percent) => {
                Err("the battery level must be between 1 and 99%".into())
            }
            _ => Ok(()),
        }
    }
}

/// One standing task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal: String,
    pub trigger: Trigger,
    #[serde(default)]
    pub paused: bool,
    /// Unix seconds.
    pub created: i64,
    #[serde(default)]
    pub last_run: Option<i64>,
    #[serde(default)]
    pub runs: u32,
    /// Battery tasks: false after firing until the battery recovers.
    #[serde(default = "yes")]
    pub armed: bool,
}

fn yes() -> bool {
    true
}

/// The battery as the system reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// All tasks, as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub tasks: Vec<Task>,
    /// Quiet hours, start and end hour (`[22, 7]`): recurring tasks wait
    /// until they end (`friday tasks quiet 22-7`). Reminders and battery
    /// alerts still come.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet: Option<[u32; 2]>,
    /// Most task runs in a day (`friday tasks limit 50`); unset: the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_cap: Option<u32>,
    /// The day the runs below were counted on, and how many.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran_on: Option<String>,
    #[serde(default)]
    pub ran_today: u32,
}

/// Task runs a day when the person set no limit: enough for any real use,
/// and a stop for a task gone wrong (free models have daily limits too).
pub const DEFAULT_DAILY_CAP: u32 = 50;

/// Whether `hour` is inside quiet hours `[start, end]` (which may cross midnight).
fn in_quiet(hour: u32, [start, end]: [u32; 2]) -> bool {
    if start <= end {
        (start..end).contains(&hour)
    } else {
        hour >= start || hour < end
    }
}

/// Where the tasks live: beside `state.json`.
pub fn store_path() -> PathBuf {
    crate::shell_state::ShellState::default_path().with_file_name("standing.json")
}

impl Store {
    /// A missing or unreadable file is an empty list: nothing runs by accident.
    pub fn load(path: &Path) -> Store {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Writes the file whole, owner-only, replacing it in one step.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        {
            use std::io::Write;
            let mut f = opts.open(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        }
        std::fs::rename(tmp, path)
    }

    /// Adds a task. The goal must be something to do, not another schedule.
    pub fn add(&mut self, goal: &str, trigger: Trigger, now: i64) -> Result<&Task, String> {
        let goal = goal.split_whitespace().collect::<Vec<_>>().join(" ");
        if goal.is_empty() {
            return Err("say what FRIDAY should do".into());
        }
        if goal.chars().count() > MAX_GOAL {
            return Err(format!("keep it under {MAX_GOAL} characters"));
        }
        if looks_like_schedule(&goal) {
            return Err("a standing task can't create another one; say only what to do".into());
        }
        trigger.valid()?;
        if self.tasks.len() >= MAX_TASKS {
            return Err(format!(
                "there are already {MAX_TASKS} standing tasks; delete one first"
            ));
        }
        let id = (1..)
            .map(|n| format!("t{n}"))
            .find(|id| self.tasks.iter().all(|t| &t.id != id))
            .expect("some id is free");
        self.tasks.push(Task {
            id,
            goal,
            trigger,
            paused: false,
            created: now,
            last_run: None,
            runs: 0,
            armed: true,
        });
        Ok(self.tasks.last().expect("just pushed"))
    }
}

/// Whether a goal is itself a schedule ("every day ...", "when my battery ...",
/// "remind me ...": run, it would make another task).
fn looks_like_schedule(goal: &str) -> bool {
    let g = goal.to_lowercase();
    g.starts_with("remind me")
        || g.starts_with("every ")
        || g.starts_with("each ")
        || g.starts_with("when ")
        || g.contains(" every day")
        || g.contains(" every morning")
        || g.contains(" every hour")
        || g.starts_with("remind me every")
}

fn local_naive(ts: i64) -> NaiveDateTime {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.naive_local())
        .unwrap_or_default()
}

/// Whether `task` is due. `now` is local wall-clock time, `now_ts` the same
/// moment in unix seconds. Pure: nothing is changed.
pub fn due(task: &Task, now: NaiveDateTime, now_ts: i64, battery: Option<Battery>) -> bool {
    if task.paused {
        return false;
    }
    let last = task.last_run.unwrap_or(task.created);
    match &task.trigger {
        Trigger::Daily {
            hour,
            minute,
            weekdays_only,
        } => {
            if *weekdays_only && matches!(now.weekday(), Weekday::Sat | Weekday::Sun) {
                return false;
            }
            let Some(at) = now.date().and_hms_opt(*hour as u32, *minute as u32, 0) else {
                return false;
            };
            now >= at && now < at + CATCH_UP && local_naive(last) < at
        }
        Trigger::Every { minutes } => now_ts - last >= i64::from(*minutes) * 60,
        Trigger::BatteryBelow { percent } => {
            task.armed && battery.is_some_and(|b| !b.charging && b.percent < *percent)
        }
        // Missed while the computer was off: still said if at most CATCH_UP late.
        Trigger::Once { at } => {
            task.runs == 0 && now_ts >= *at && now_ts < *at + CATCH_UP.num_seconds()
        }
    }
}

/// A one-off's moment from the model's `{"kind": "at", "hour", "minute",
/// "days"}` (the next such local time, `days` later) or `{"kind": "in",
/// "minutes"}`. Other triggers pass through unchanged.
fn resolve_once(when: serde_json::Value, now: NaiveDateTime, now_ts: i64) -> serde_json::Value {
    let num = |k: &str| when.get(k).and_then(serde_json::Value::as_i64);
    let at = match when.get("kind").and_then(serde_json::Value::as_str) {
        Some("in") => num("minutes")
            .filter(|m| (1..=7 * 24 * 60).contains(m))
            .map(|m| now_ts + m * 60),
        Some("at") => (|| {
            let (h, m) = (
                u32::try_from(num("hour")?).ok()?,
                u32::try_from(num("minute").unwrap_or(0)).ok()?,
            );
            let days = num("days").unwrap_or(0).clamp(0, 7);
            let mut t = now.date().and_hms_opt(h, m, 0)? + Span::days(days);
            if t <= now {
                t += Span::days(1);
            }
            // A time the clocks skip (spring forward) is said an hour later.
            Local
                .from_local_datetime(&t)
                .earliest()
                .or_else(|| Local.from_local_datetime(&(t + Span::hours(1))).earliest())
                .map(|d| d.timestamp())
        })(),
        _ => return when,
    };
    // Unreadable: a kind serde refuses, so the tool says it couldn't tell when.
    at.map_or(
        serde_json::json!({"kind": "?"}),
        |at| serde_json::json!({"kind": "once", "at": at}),
    )
}

/// Re-arms battery tasks whose battery has recovered, marks the due tasks as
/// run, and returns their `(id, goal)`. The caller saves the store.
pub fn tick(
    store: &mut Store,
    now: NaiveDateTime,
    now_ts: i64,
    battery: Option<Battery>,
) -> Vec<(String, String)> {
    let mut run = Vec::new();
    let quiet_now = store.quiet.is_some_and(|q| in_quiet(now.hour(), q));
    let today = now.date().to_string();
    let mut ran_today = if store.ran_on.as_deref() == Some(today.as_str()) {
        store.ran_today
    } else {
        0
    };
    let cap = store.daily_cap.unwrap_or(DEFAULT_DAILY_CAP);
    for t in &mut store.tasks {
        if let Trigger::BatteryBelow { percent } = t.trigger {
            let low = battery.is_some_and(|b| !b.charging && b.percent < percent);
            if !low && battery.is_some() {
                t.armed = true;
            }
        }
        if due(t, now, now_ts, battery) {
            // Quiet hours hold recurring tasks; the day's limit holds the rest.
            let recurring = matches!(t.trigger, Trigger::Daily { .. } | Trigger::Every { .. });
            // (A reminder is never held by the limit: it would go unsaid.)
            let reminder = matches!(t.trigger, Trigger::Once { .. });
            if (quiet_now && recurring) || (ran_today >= cap && !reminder) {
                continue;
            }
            ran_today += 1;
            t.last_run = Some(now_ts);
            t.runs += 1;
            if matches!(t.trigger, Trigger::BatteryBelow { .. }) {
                t.armed = false;
            }
            run.push((t.id.clone(), t.goal.clone()));
        }
    }
    if ran_today > 0 || store.ran_on.is_some() {
        store.ran_on = Some(today);
        store.ran_today = ran_today;
    }
    // A reminder is done once said, or once too late to say.
    store.tasks.retain(|t| match t.trigger {
        Trigger::Once { at } => t.runs == 0 && now_ts < at + CATCH_UP.num_seconds(),
        _ => true,
    });
    run
}

/// The battery as Windows reports it; `None` on a machine without one.
#[cfg(windows)]
pub fn read_battery() -> Option<Battery> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    // SAFETY: GetSystemPowerStatus only fills the struct it is given.
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return None;
    }
    // 128: no battery; 255: unknown.
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery {
        percent: s.BatteryLifePercent,
        charging: s.ACLineStatus == 1,
    })
}

/// The battery from `/sys/class/power_supply` (the first one that reports a
/// capacity); `None` on a machine without one.
#[cfg(not(windows))]
pub fn read_battery() -> Option<Battery> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        let p = entry.path();
        let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        let percent: u8 = std::fs::read_to_string(p.join("capacity"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
        return Some(Battery {
            percent,
            charging: matches!(status.trim(), "Charging" | "Full"),
        });
    }
    None
}

/// Runs a due task's goal.
#[async_trait]
pub trait Runner: Send + Sync {
    async fn run(&self, goal: &str);
}

/// Runs the goal with `friday ask`, as the person's own ask would run.
pub struct FridayAsk;

#[async_trait]
impl Runner for FridayAsk {
    async fn run(&self, goal: &str) {
        let child = tokio::process::Command::new("friday")
            .arg("ask")
            .arg(goal)
            .env("FRIDAY_STANDING", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        match child {
            Ok(mut c) => {
                let _ = tokio::time::timeout(Duration::from_secs(10 * 60), c.wait()).await;
            }
            Err(e) => tracing::warn!("[standing] couldn't run friday ask: {e}"),
        }
    }
}

/// Where the daemon leaves due goals for the tray to ask (Windows).
pub fn tray_queue_path() -> PathBuf {
    store_path().with_file_name("standing-due.jsonl")
}

/// Hands the goal to the tray, one JSON line per due goal; the tray asks it
/// the way the person's own ask runs, and removes the line.
pub struct TrayQueue(pub PathBuf);

#[async_trait]
impl Runner for TrayQueue {
    async fn run(&self, goal: &str) {
        use std::io::Write;
        let line = serde_json::json!({ "goal": goal, "ts": chrono::Utc::now().timestamp() });
        let wrote = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = wrote {
            tracing::warn!("[standing] couldn't queue for the tray: {e}");
        }
    }
}

/// Checks the file every [`TICK`] and runs what is due, one task at a time.
/// The file is read each time, so `friday tasks` changes apply at once.
pub async fn run_loop(path: PathBuf, runner: std::sync::Arc<dyn Runner>) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let mut store = Store::load(&path);
        if store.tasks.is_empty() {
            continue;
        }
        let before = store.clone();
        let now = Local::now();
        let due = tick(
            &mut store,
            now.naive_local(),
            now.timestamp(),
            read_battery(),
        );
        if store != before {
            // Saved before running, so a slow or crashing run is never repeated.
            // The file is re-read first: `friday tasks` may have just edited it.
            let mut latest = Store::load(&path);
            for t in &store.tasks {
                if let Some(l) = latest.tasks.iter_mut().find(|l| l.id == t.id) {
                    l.last_run = t.last_run;
                    l.runs = t.runs;
                    l.armed = t.armed;
                }
            }
            latest.ran_on = store.ran_on.clone();
            latest.ran_today = store.ran_today;
            if let Err(e) = latest.save(&path) {
                tracing::warn!("[standing] couldn't save {}: {e}", path.display());
                continue;
            }
        }
        for (id, goal) in due {
            tracing::info!("[standing] running {id}");
            runner.run(&goal).await;
        }
    }
}

/// A standing task made in one click (`friday tasks recipes`, Settings):
/// each asks for something FRIDAY already does.
pub struct Recipe {
    pub id: &'static str,
    pub what: &'static str,
    pub goal: &'static str,
    pub trigger: Trigger,
}

pub const RECIPES: [Recipe; 5] = [
    Recipe {
        id: "morning-brief",
        what: "Weekday mornings at 9: what you missed (agents, tasks, reminders)",
        goal: "what did I miss?",
        trigger: Trigger::Daily {
            hour: 9,
            minute: 0,
            weekdays_only: true,
        },
    },
    Recipe {
        id: "weather",
        what: "Every morning at 7:30: today's weather",
        goal: "what's the weather today?",
        trigger: Trigger::Daily {
            hour: 7,
            minute: 30,
            weekdays_only: false,
        },
    },
    Recipe {
        id: "plug-in",
        what: "When the battery is under 20%: a reminder to plug in",
        goal: "tell me the battery is low and to plug in the charger",
        trigger: Trigger::BatteryBelow { percent: 20 },
    },
    Recipe {
        id: "health-check",
        what: "Once a week: how the computer is doing (disk, memory, updates)",
        goal: "how is my computer doing?",
        trigger: Trigger::Every {
            minutes: 7 * 24 * 60,
        },
    },
    Recipe {
        id: "tech-news",
        what: "Every evening at 6: the day's top tech news",
        goal: "what's the top tech news today?",
        trigger: Trigger::Daily {
            hour: 18,
            minute: 0,
            weekdays_only: false,
        },
    },
];

/// Adds the recipe `id` to the store at `path`; the plain answer.
pub fn add_recipe(path: &Path, id: &str, now: i64) -> Result<String, String> {
    let r = RECIPES.iter().find(|r| r.id == id).ok_or_else(|| {
        let ids: Vec<&str> = RECIPES.iter().map(|r| r.id).collect();
        format!("there's no recipe {id}; there are: {}", ids.join(", "))
    })?;
    let mut store = Store::load(path);
    if store
        .tasks
        .iter()
        .any(|t| t.goal == r.goal && t.trigger == r.trigger)
    {
        return Ok(format!("Already set up: {}.", r.what));
    }
    let added = store.add(r.goal, r.trigger.clone(), now)?.id.clone();
    store
        .save(path)
        .map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    Ok(format!("Added {added}: {}.", r.what))
}

/// The capability the planner routes "every morning ..." goals to.
pub const CAPABILITY: &str = "standing.tasks";

/// Whether this machine can run standing tasks: Linux with the `friday` CLI
/// or Windows with the tray (which run them), and the owner hasn't turned
/// them off (`FRIDAY_STANDING_TASKS=0`).
pub fn enabled() -> bool {
    (cfg!(target_os = "linux") || cfg!(windows))
        && std::env::var("FRIDAY_STANDING_TASKS")
            .map(|v| v != "0")
            .unwrap_or(true)
}

const MODEL_TIMEOUT: Duration = Duration::from_secs(45);

const GUIDE: &str = r#"Turn the person's request into a standing task: something FRIDAY does again and again on a schedule, or once at a time (a reminder).
Reply with ONLY one JSON object, nothing else:
  {"op": "add", "goal": "<what to do, as a short instruction, without the schedule>", "when": <trigger>}
  {"op": "list"}                      the person asked what standing tasks they have
  {"op": "none", "say": "<why not>"}  the request is not a standing task
A trigger is one of:
  {"kind": "daily", "hour": 8, "minute": 0}                       every day at that local time (24-hour); add "weekdays_only": true for Monday to Friday
  {"kind": "every", "minutes": 90}                                 repeat every so many minutes (at least 5)
  {"kind": "battery_below", "percent": 20}                         once each time the battery falls under that level while unplugged
  {"kind": "at", "hour": 12, "minute": 57}                         once, at the next such local time (24-hour); add "days": 1 for tomorrow
  {"kind": "in", "minutes": 20}                                    once, that many minutes from now
For a reminder the goal is what to say, like "say: Time to dance." ("remind me at 1 to dance").
"morning" is 8:00, "noon" 12:00, "evening" 18:00, "night" 21:00 unless a time is given.
The goal must not mention the schedule: "every morning tell me the weather" -> goal "tell me the weather"."#;

fn first_json(reply: &str) -> Option<serde_json::Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

/// Creates and lists standing tasks. Making one is the person's own explicit
/// request, so it needs no extra confirmation; what it later does runs under
/// the usual consent classes.
pub struct StandingTool {
    model: std::sync::Arc<dyn friday_investigation::LlmStrategyClient>,
    path: PathBuf,
    /// Asks first when the goal carries quoted text (see `ConfirmationGate::quoted`).
    gate: Option<std::sync::Arc<std::sync::Mutex<crate::goal_runner::ConfirmationGate>>>,
}

impl StandingTool {
    pub fn new(
        model: std::sync::Arc<dyn friday_investigation::LlmStrategyClient>,
        path: PathBuf,
    ) -> Self {
        Self {
            model,
            path,
            gate: None,
        }
    }

    /// Asks first, through the goal's confirmation gate, when the goal
    /// carries quoted text: a page or selection could plant a task that runs
    /// unattended later.
    pub(crate) fn with_gate(
        mut self,
        gate: std::sync::Arc<std::sync::Mutex<crate::goal_runner::ConfirmationGate>>,
    ) -> Self {
        self.gate = Some(gate);
        self
    }
}

fn describe_all(store: &Store) -> String {
    if store.tasks.is_empty() {
        return "No standing tasks yet. Ask for one, like \"every morning at 8 tell me the weather\".".into();
    }
    store
        .tasks
        .iter()
        .map(|t| {
            format!(
                "{} {} · {}{}",
                t.id,
                t.trigger.describe(),
                t.goal,
                if t.paused { " (paused)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl friday_goal_execution::Tool for StandingTool {
    fn name(&self) -> &str {
        "standing"
    }

    fn description(&self) -> &str {
        "Creates and lists standing tasks that run on a schedule or a battery level (standing.tasks)"
    }

    fn capabilities(&self) -> Vec<friday_goal_execution::Capability> {
        vec![friday_goal_execution::Capability::Custom(CAPABILITY.into())]
    }

    async fn execute(
        &self,
        action: &friday_goal_execution::Action,
        _ctx: &friday_goal_execution::ExecutionContext,
    ) -> friday_goal_execution::ToolResult {
        use friday_goal_execution::ToolResult;
        let goal = friday_goal_execution::planning::goal_segment(&action.instruction)
            .trim()
            .to_string();
        if let Some(gate) = &self.gate {
            if let Err(err) = gate.lock().unwrap().check_if_quoted("standing", &goal) {
                return err;
            }
        }
        let prompt = format!("{GUIDE}\n\nThe person's request: {goal}\n\nYour reply (JSON only):");
        let reply =
            match tokio::time::timeout(MODEL_TIMEOUT, self.model.complete_json(&prompt)).await {
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
            return ToolResult::error("couldn't tell what schedule was meant");
        };
        match choice.get("op").and_then(serde_json::Value::as_str) {
            Some("list") => ToolResult::success(describe_all(&Store::load(&self.path))),
            Some("add") => {
                let what = choice
                    .get("goal")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let now = chrono::Utc::now().timestamp();
                let Some(trigger) = choice
                    .get("when")
                    .cloned()
                    .map(|w| resolve_once(w, Local::now().naive_local(), now))
                    .and_then(|w| serde_json::from_value::<Trigger>(w).ok())
                else {
                    return ToolResult::error(
                        "couldn't tell when that should run; say a time, an interval or a battery level",
                    );
                };
                let mut store = Store::load(&self.path);
                let made = match store.add(what, trigger, now) {
                    Ok(t) => format!("{}: {} · {}", t.id, t.trigger.describe(), t.goal),
                    Err(e) => return ToolResult::error(e),
                };
                if let Err(e) = store.save(&self.path) {
                    return ToolResult::error(format!("couldn't save the task: {e}"));
                }
                let manage = if cfg!(windows) {
                    "in FRIDAY's Settings, under Tasks"
                } else {
                    "with: friday tasks"
                };
                ToolResult::success(format!(
                    "Done. Standing task {made}\nSee, pause or delete your tasks {manage}"
                ))
            }
            _ => {
                let say = choice
                    .get("say")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                ToolResult::error(if say.is_empty() {
                    "That is not a standing task.".to_string()
                } else {
                    say.chars().take(300).collect()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recipe_is_a_valid_task_and_adding_one_twice_adds_it_once() {
        let path = std::env::temp_dir().join(format!(
            "friday-recipes-{}-{}.json",
            std::process::id(),
            rand::random::<u32>()
        ));
        for r in &RECIPES {
            assert!(r.trigger.valid().is_ok(), "{}", r.id);
            assert!(!looks_like_schedule(r.goal), "{}", r.id);
            assert!(
                add_recipe(&path, r.id, 1_000).unwrap().starts_with("Added"),
                "{}",
                r.id
            );
        }
        assert!(add_recipe(&path, "weather", 1_000)
            .unwrap()
            .starts_with("Already"));
        assert_eq!(Store::load(&path).tasks.len(), RECIPES.len());
        assert!(add_recipe(&path, "nope", 1_000)
            .unwrap_err()
            .contains("weather"));
        let _ = std::fs::remove_file(&path);
    }
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    fn ts(n: NaiveDateTime) -> i64 {
        Local
            .from_local_datetime(&n)
            .earliest()
            .unwrap()
            .timestamp()
    }

    fn task(trigger: Trigger, created: NaiveDateTime) -> Task {
        Task {
            id: "t1".into(),
            goal: "tell me the weather".into(),
            trigger,
            paused: false,
            created: ts(created),
            last_run: None,
            runs: 0,
            armed: true,
        }
    }

    fn daily(h: u8, m: u8) -> Trigger {
        Trigger::Daily {
            hour: h,
            minute: m,
            weekdays_only: false,
        }
    }

    // 2026-11-16 is a Monday.
    #[test]
    fn a_daily_task_runs_once_when_its_time_comes() {
        let mut t = task(daily(8, 0), at(2026, 11, 15, 20, 0));
        assert!(!due(&t, at(2026, 11, 16, 7, 59), 0, None));
        let now = at(2026, 11, 16, 8, 0);
        assert!(due(&t, now, ts(now), None));
        t.last_run = Some(ts(now));
        assert!(!due(&t, at(2026, 11, 16, 8, 20), 0, None));
        assert!(due(&t, at(2026, 11, 17, 8, 1), 0, None));
    }

    #[test]
    fn a_task_made_after_todays_time_waits_for_tomorrow() {
        let t = task(daily(8, 0), at(2026, 11, 16, 9, 0));
        assert!(!due(&t, at(2026, 11, 16, 9, 5), 0, None));
        assert!(due(&t, at(2026, 11, 17, 8, 0), 0, None));
    }

    #[test]
    fn a_missed_morning_runs_late_but_not_in_the_evening() {
        let t = task(daily(8, 0), at(2026, 11, 14, 12, 0));
        assert!(due(&t, at(2026, 11, 16, 11, 59), 0, None));
        assert!(!due(&t, at(2026, 11, 16, 14, 1), 0, None));
    }

    #[test]
    fn weekday_tasks_skip_the_weekend() {
        let t = task(
            Trigger::Daily {
                hour: 9,
                minute: 0,
                weekdays_only: true,
            },
            at(2026, 11, 10, 0, 0),
        );
        assert!(!due(&t, at(2026, 11, 14, 9, 5), 0, None)); // Saturday
        assert!(!due(&t, at(2026, 11, 15, 9, 5), 0, None)); // Sunday
        assert!(due(&t, at(2026, 11, 16, 9, 5), 0, None)); // Monday
    }

    #[test]
    fn a_repeat_counts_from_the_last_run() {
        let created = at(2026, 11, 16, 8, 0);
        let mut t = task(Trigger::Every { minutes: 60 }, created);
        assert!(!due(&t, created, ts(created) + 59 * 60, None));
        assert!(due(&t, created, ts(created) + 60 * 60, None));
        t.last_run = Some(ts(created) + 3600);
        assert!(!due(&t, created, ts(created) + 3600 + 59 * 60, None));
    }

    #[test]
    fn a_battery_task_fires_once_per_drop_and_rearms_when_it_recovers() {
        let now = at(2026, 11, 16, 8, 0);
        let mut store = Store::default();
        store
            .add(
                "tell me the battery is low",
                Trigger::BatteryBelow { percent: 20 },
                ts(now),
            )
            .unwrap();
        let low = Some(Battery {
            percent: 15,
            charging: false,
        });
        assert_eq!(tick(&mut store, now, ts(now), low).len(), 1);
        assert!(
            tick(&mut store, now, ts(now), low).is_empty(),
            "not again while low"
        );
        let plugged = Some(Battery {
            percent: 15,
            charging: true,
        });
        assert!(tick(&mut store, now, ts(now), plugged).is_empty());
        assert_eq!(
            tick(&mut store, now, ts(now), low).len(),
            1,
            "plugging in re-armed it"
        );
        // No battery reading (a desktop): nothing fires.
        assert!(tick(&mut store, now, ts(now), None).is_empty());
        let high = Some(Battery {
            percent: 80,
            charging: false,
        });
        assert!(tick(&mut store, now, ts(now), high).is_empty());
    }

    #[tokio::test]
    async fn the_tray_queue_gets_one_line_per_due_goal() {
        let dir =
            std::env::temp_dir().join(format!("friday-standing-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let q = TrayQueue(dir.join("standing-due.jsonl"));
        let _ = std::fs::remove_file(&q.0);
        q.run("tell me the weather").await;
        q.run("check my mail").await;
        let text = std::fs::read_to_string(&q.0).unwrap();
        let goals: Vec<String> = text
            .lines()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(l).unwrap()["goal"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(goals, ["tell me the weather", "check my mail"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reminder_resolves_to_the_next_such_time() {
        let now = at(2026, 11, 22, 12, 30);
        let n = ts(now);
        let once = |w| serde_json::from_value::<Trigger>(resolve_once(w, now, n)).ok();
        let later = ts(at(2026, 11, 22, 12, 57));
        assert_eq!(
            once(serde_json::json!({"kind": "at", "hour": 12, "minute": 57})),
            Some(Trigger::Once { at: later })
        );
        // Already past today: tomorrow.
        let tomorrow = ts(at(2026, 11, 23, 9, 0));
        assert_eq!(
            once(serde_json::json!({"kind": "at", "hour": 9, "minute": 0})),
            Some(Trigger::Once { at: tomorrow })
        );
        assert_eq!(
            once(serde_json::json!({"kind": "at", "hour": 9, "minute": 0, "days": 1})),
            Some(Trigger::Once { at: tomorrow })
        );
        assert_eq!(
            once(serde_json::json!({"kind": "in", "minutes": 20})),
            Some(Trigger::Once { at: n + 1200 })
        );
        // Nonsense isn't a trigger; the others pass through.
        assert_eq!(
            once(serde_json::json!({"kind": "at", "hour": 25, "minute": 0})),
            None
        );
        assert_eq!(once(serde_json::json!({"kind": "in", "minutes": 0})), None);
        assert_eq!(
            once(serde_json::json!({"kind": "every", "minutes": 60})),
            Some(Trigger::Every { minutes: 60 })
        );
    }

    #[test]
    fn a_reminder_runs_once_then_goes() {
        let made = at(2026, 11, 22, 12, 30);
        let due_at = ts(at(2026, 11, 22, 12, 57));
        let mut store = Store {
            tasks: vec![task(Trigger::Once { at: due_at }, made)],
            ..Default::default()
        };
        store.tasks[0].goal = "say: Time to dance.".into();
        assert!(tick(&mut store, at(2026, 11, 22, 12, 56), due_at - 60, None).is_empty());
        assert_eq!(store.tasks.len(), 1);
        let run = tick(&mut store, at(2026, 11, 22, 12, 57), due_at, None);
        assert_eq!(
            run,
            vec![("t1".to_string(), "say: Time to dance.".to_string())]
        );
        assert!(store.tasks.is_empty(), "said once, then removed");
        // Missed by more than six hours (the computer was off): dropped unsaid.
        let mut store = Store {
            tasks: vec![task(Trigger::Once { at: due_at }, made)],
            ..Default::default()
        };
        assert!(tick(&mut store, at(2026, 11, 22, 19, 0), due_at + 7 * 3600, None).is_empty());
        assert!(store.tasks.is_empty());
    }

    #[test]
    fn a_reminder_goal_cant_remind_again() {
        let mut s = Store::default();
        assert!(s
            .add("remind me to dance", Trigger::Once { at: 10 }, 0)
            .is_err());
        assert!(s
            .add("say: Time to dance.", Trigger::Once { at: 10 }, 0)
            .is_ok());
    }

    #[test]
    fn a_paused_task_never_runs() {
        let mut t = task(Trigger::Every { minutes: 5 }, at(2026, 11, 16, 8, 0));
        t.paused = true;
        assert!(!due(
            &t,
            at(2026, 11, 16, 9, 0),
            ts(at(2026, 11, 16, 9, 0)),
            None
        ));
    }

    /// Quiet hours hold recurring tasks until morning (a missed one catches
    /// up then), never a reminder; the day's limit holds everything.
    #[test]
    fn quiet_hours_and_the_days_limit_hold_tasks() {
        let created = at(2026, 11, 16, 20, 0);
        let mut store = Store {
            quiet: Some([22, 7]),
            ..Default::default()
        };
        store
            .add("tell me the news", daily(6, 30), ts(created))
            .unwrap();
        let reminder = ts(at(2026, 11, 16, 23, 0));
        store
            .add("say: Bed.", Trigger::Once { at: reminder }, ts(created))
            .unwrap();
        let night = at(2026, 11, 16, 23, 0);
        let run = tick(&mut store, night, ts(night), None);
        assert_eq!(
            run,
            vec![("t2".to_string(), "say: Bed.".to_string())],
            "a reminder still comes"
        );
        let early = at(2026, 11, 17, 6, 31);
        assert!(
            tick(&mut store, early, ts(early), None).is_empty(),
            "6:30 is quiet"
        );
        let morning = at(2026, 11, 17, 7, 0);
        assert_eq!(
            tick(&mut store, morning, ts(morning), None).len(),
            1,
            "caught up after"
        );

        let mut store = Store {
            daily_cap: Some(2),
            ..Default::default()
        };
        store
            .add(
                "check the build",
                Trigger::Every { minutes: 5 },
                ts(created),
            )
            .unwrap();
        let mut ran = 0;
        for m in (0..60).step_by(5) {
            let now = at(2026, 11, 16, 21, m);
            ran += tick(&mut store, now, ts(now), None).len();
        }
        assert_eq!(ran, 2, "no more than the day's limit");
        let next_day = at(2026, 11, 17, 9, 0);
        assert_eq!(
            tick(&mut store, next_day, ts(next_day), None).len(),
            1,
            "a new day, a new count"
        );
        assert!(
            in_quiet(23, [22, 7])
                && in_quiet(3, [22, 7])
                && !in_quiet(7, [22, 7])
                && in_quiet(13, [12, 14])
        );
    }

    #[test]
    fn tick_marks_what_it_returns_as_run() {
        let created = at(2026, 11, 16, 7, 0);
        let mut store = Store::default();
        store
            .add("play the news", daily(8, 0), ts(created))
            .unwrap();
        let now = at(2026, 11, 16, 8, 1);
        assert_eq!(
            tick(&mut store, now, ts(now), None),
            vec![("t1".into(), "play the news".into())]
        );
        assert_eq!(store.tasks[0].runs, 1);
        assert!(tick(&mut store, now, ts(now), None).is_empty());
    }

    #[test]
    fn adding_checks_the_goal_and_the_trigger() {
        let mut s = Store::default();
        assert!(s.add("  ", daily(8, 0), 0).is_err());
        assert!(s.add(&"x".repeat(MAX_GOAL + 1), daily(8, 0), 0).is_err());
        assert!(s.add("every day tell me", daily(8, 0), 0).is_err());
        assert!(s
            .add("when my battery is low tell me", daily(8, 0), 0)
            .is_err());
        assert!(s.add("hi", daily(24, 0), 0).is_err());
        assert!(s.add("hi", Trigger::Every { minutes: 1 }, 0).is_err());
        assert!(s
            .add("hi", Trigger::BatteryBelow { percent: 0 }, 0)
            .is_err());
        assert!(s.tasks.is_empty());
        for _ in 0..MAX_TASKS {
            s.add("hi", daily(8, 0), 0).unwrap();
        }
        assert!(s
            .add("hi", daily(8, 0), 0)
            .unwrap_err()
            .contains("delete one"));
        let ids: std::collections::HashSet<_> = s.tasks.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids.len(), MAX_TASKS);
    }

    #[test]
    fn a_deleted_id_is_reused_not_duplicated() {
        let mut s = Store::default();
        s.add("a", daily(8, 0), 0).unwrap();
        s.add("b", daily(9, 0), 0).unwrap();
        s.tasks.retain(|t| t.id != "t1");
        assert_eq!(s.add("c", daily(10, 0), 0).unwrap().id, "t1");
    }

    #[test]
    fn the_file_round_trips_is_private_and_a_bad_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("friday-standing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("standing.json");
        assert_eq!(Store::load(&path), Store::default());
        let mut s = Store::default();
        s.add("tell me the weather", daily(8, 30), 5).unwrap();
        s.save(&path).unwrap();
        assert_eq!(Store::load(&path), s);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Store::load(&path), Store::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cli_can_edit_the_file_with_plain_json() {
        // `friday tasks` writes this shape with jq: keep it stable.
        let text = r#"{"tasks":[{"id":"t1","goal":"hi","trigger":{"kind":"daily","hour":8,"minute":0},"created":1}]}"#;
        let s: Store = serde_json::from_str(text).unwrap();
        assert!(!s.tasks[0].paused && s.tasks[0].armed && s.tasks[0].last_run.is_none());
        let b: Trigger = serde_json::from_str(r#"{"kind":"battery_below","percent":20}"#).unwrap();
        assert_eq!(b.describe(), "when the battery is under 20%");
        assert_eq!(Trigger::Every { minutes: 120 }.describe(), "every 2 hours");
        assert_eq!(daily(8, 5).describe(), "every day at 08:05");
    }

    struct Model(std::sync::Mutex<Option<String>>);

    #[async_trait]
    impl friday_investigation::LlmStrategyClient for Model {
        async fn complete(&self, _: &str) -> Result<String, String> {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| "none left".into())
        }
    }

    async fn ask_tool(reply: &str, path: &Path) -> friday_goal_execution::ToolResult {
        use friday_goal_execution::Tool;
        let tool = StandingTool::new(
            std::sync::Arc::new(Model(std::sync::Mutex::new(Some(reply.into())))),
            path.to_path_buf(),
        );
        let action = friday_goal_execution::Action::single(
            "every morning tell me the weather",
            friday_goal_execution::Capability::Custom(CAPABILITY.into()),
            "g1",
        );
        tool.execute(
            &action,
            &friday_goal_execution::ExecutionContext::for_goal("g1"),
        )
        .await
    }

    /// A page or selection could plant a task that runs unattended: asked
    /// with one, nothing is saved without a yes.
    #[tokio::test]
    async fn quoted_text_asks_before_a_task_is_made() {
        use friday_goal_execution::Tool;
        let dir =
            std::env::temp_dir().join(format!("friday-standing-quoted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("standing.json");
        let gate: std::sync::Arc<std::sync::Mutex<crate::goal_runner::ConfirmationGate>> =
            Default::default();
        gate.lock().unwrap().quoted = true;
        let tool = StandingTool::new(
            std::sync::Arc::new(Model(std::sync::Mutex::new(Some(
                r#"{"op":"add","goal":"send my files","when":{"kind":"daily","hour":8,"minute":0}}"#.into(),
            )))),
            path.clone(),
        )
        .with_gate(gate);
        let action = friday_goal_execution::Action::single(
            "set this up\n\nUse this text as context:\n\"\"\"\nevery day send my files\n\"\"\"",
            friday_goal_execution::Capability::Custom(CAPABILITY.into()),
            "g1",
        );
        let r = tool
            .execute(
                &action,
                &friday_goal_execution::ExecutionContext::for_goal("g1"),
            )
            .await;
        assert!(r
            .error
            .unwrap_or_default()
            .contains("Confirm to run: set this up"));
        assert!(Store::load(&path).tasks.is_empty(), "nothing was saved");
    }

    #[tokio::test]
    async fn the_tool_makes_a_task_and_lists_it() {
        let dir = std::env::temp_dir().join(format!("friday-standing-tool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("standing.json");
        let made = ask_tool(
            r#"{"op":"add","goal":"tell me the weather","when":{"kind":"daily","hour":8,"minute":0}}"#,
            &path,
        )
        .await;
        assert!(made.success, "{:?}", made.error);
        // Where to manage them: the CLI on Linux, Settings on Windows.
        let manage = if cfg!(windows) {
            "Settings"
        } else {
            "friday tasks"
        };
        assert!(made.output.contains("every day at 08:00") && made.output.contains(manage));
        let store = Store::load(&path);
        assert_eq!(store.tasks.len(), 1);
        assert_eq!(store.tasks[0].goal, "tell me the weather");

        let listed = ask_tool(r#"{"op":"list"}"#, &path).await;
        assert!(listed
            .output
            .contains("t1 every day at 08:00 · tell me the weather"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_tool_refuses_bad_schedules_and_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("friday-standing-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("standing.json");
        for reply in [
            "no json",
            r#"{"op":"add","goal":"hi"}"#,
            r#"{"op":"add","goal":"hi","when":{"kind":"every","minutes":1}}"#,
            r#"{"op":"add","goal":"hi","when":{"kind":"hourly"}}"#,
            r#"{"op":"add","goal":"every day say hi","when":{"kind":"daily","hour":8,"minute":0}}"#,
            r#"{"op":"add","goal":"","when":{"kind":"daily","hour":8,"minute":0}}"#,
            r#"{"op":"none","say":"that isn't a schedule"}"#,
        ] {
            let res = ask_tool(reply, &path).await;
            assert!(!res.success, "{reply}");
        }
        assert!(!path.exists());
        let empty = ask_tool(r#"{"op":"list"}"#, &path).await;
        assert!(empty.output.contains("No standing tasks yet"));
    }
}
