//! Hyprland screen + focus sensors — FRIDAY OS's counterparts of
//! [`WindowsScreenSensor`](crate::screen) and
//! [`WindowsFocusSensor`](crate::windows_focus). They emit the same events
//! (`Topic::Screen`, `Topic::WindowFocusChanged`) with the same payloads, so
//! everything downstream of the sensor loop is platform-blind.
//!
//! ```text
//! hyprctl -j activewindow           → identity + geometry + state
//! grim -s 0.25 -t ppm -g <window> - → content_hash (FNV-1a of the pixels)
//! ```
//!
//! The pixels are only hashed — never stored or sent. A quarter-scale capture
//! keeps the per-poll cost low while still flipping the hash on any visible
//! change. Capture failure degrades to `content_hash: None`, exactly like the
//! Windows sensor.
//!
//! Commands run through an injected [`CommandFn`] so the sensors are proven
//! against scripted output; the daemon passes a runner that also rediscovers
//! the Wayland session (a systemd service may start before the compositor).

use std::sync::Arc;

use async_trait::async_trait;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;
use serde_json::Value;

use crate::screen::{fnv1a64, observe, ScreenObservation};
use crate::Sensor;

/// Run `program args…` and return its stdout, or `None` if it could not run
/// or exited non-zero.
pub type CommandFn = Arc<dyn Fn(&str, &[&str]) -> Option<Vec<u8>> + Send + Sync>;

/// The focused Hyprland client, or `None` when nothing is focused or there
/// is no Hyprland session.
pub fn active_window(run: &CommandFn) -> Option<Value> {
    let out = run("hyprctl", &["-j", "activewindow"])?;
    let window: Value = serde_json::from_slice(&out).ok()?;
    window["address"].as_str().filter(|a| !a.is_empty())?;
    Some(window)
}

/// Map a Hyprland client to the platform-neutral observation.
pub fn observation_from(client: &Value, content_hash: Option<u64>) -> ScreenObservation {
    let num = |key: &str, i: usize| client[key][i].as_i64().unwrap_or(0) as i32;
    let state = if client["hidden"].as_bool() == Some(true) {
        "minimized"
    } else if client["fullscreen"].as_i64().unwrap_or(0) != 0 {
        "maximized"
    } else {
        "normal"
    };
    ScreenObservation {
        title: client["title"].as_str().unwrap_or_default().to_string(),
        pid: pid_of(client),
        process: client["class"].as_str().unwrap_or_default().to_string(),
        bounds: (num("at", 0), num("at", 1), num("size", 0), num("size", 1)),
        state: state.to_string(),
        content_hash,
    }
}

fn pid_of(client: &Value) -> u32 {
    client["pid"].as_i64().filter(|p| *p > 0).unwrap_or(0) as u32
}

/// Polls the focused window and emits a `Topic::Screen` event on a focus
/// switch, a geometry/state change, or a content change.
pub struct HyprlandScreenSensor {
    run: CommandFn,
    last: Option<ScreenObservation>,
    /// Whether to read the window's pixels for `content_hash` (off for the
    /// live daemon unless the person opts in: screen capture needs consent).
    pixels: bool,
}

impl HyprlandScreenSensor {
    pub fn new(run: CommandFn) -> Self {
        Self {
            run,
            last: None,
            pixels: true,
        }
    }

    /// Observe the focused window's identity and geometry only: no `grim`,
    /// so `content_hash` is always `None`.
    pub fn without_pixels(mut self) -> Self {
        self.pixels = false;
        self
    }

    fn content_hash(&self, client: &Value) -> Option<u64> {
        if !self.pixels {
            return None;
        }
        let (x, y) = (client["at"][0].as_i64()?, client["at"][1].as_i64()?);
        let (w, h) = (client["size"][0].as_i64()?, client["size"][1].as_i64()?);
        if w <= 0 || h <= 0 {
            return None;
        }
        let geometry = format!("{x},{y} {w}x{h}");
        let pixels = (self.run)("grim", &["-s", "0.25", "-t", "ppm", "-g", &geometry, "-"])?;
        (!pixels.is_empty()).then(|| fnv1a64(&pixels))
    }
}

#[async_trait]
impl Sensor for HyprlandScreenSensor {
    fn name(&self) -> &str {
        "hyprland_screen"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.screen"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let Some(client) = active_window(&self.run) else {
            return Vec::new();
        };
        let observation = observation_from(&client, self.content_hash(&client));
        observe(&mut self.last, observation).into_iter().collect()
    }
}

/// Emits `Topic::WindowFocusChanged` whenever the focused window's title or
/// pid changes (including focus moving to nothing).
pub struct HyprlandFocusSensor {
    run: CommandFn,
    last: Option<(String, u32)>,
}

impl HyprlandFocusSensor {
    pub fn new(run: CommandFn) -> Self {
        Self { run, last: None }
    }
}

#[async_trait]
impl Sensor for HyprlandFocusSensor {
    fn name(&self) -> &str {
        "hyprland_focus"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.window"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let focus = active_window(&self.run)
            .map(|c| {
                (
                    c["title"].as_str().unwrap_or_default().to_string(),
                    pid_of(&c),
                )
            })
            .unwrap_or_default();
        if self.last.as_ref() == Some(&focus) {
            return Vec::new();
        }
        self.last = Some(focus.clone());
        vec![WorkspaceEvent::new(
            Topic::WindowFocusChanged,
            serde_json::json!({ "title": focus.0, "pid": focus.1 }),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// Scripted commands: `hyprctl` answers from a queue (last entry repeats),
    /// `grim` returns fixed pixels. Records every call.
    fn scripted(
        windows: Vec<Option<Value>>,
        pixels: Option<&'static [u8]>,
    ) -> (CommandFn, Arc<Mutex<Vec<String>>>) {
        let queue = Arc::new(Mutex::new(windows));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        let run: CommandFn = Arc::new(move |program: &str, args: &[&str]| {
            log.lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            match program {
                "hyprctl" => {
                    let mut q = queue.lock().unwrap();
                    let next = if q.len() > 1 {
                        q.remove(0)
                    } else {
                        q[0].clone()
                    };
                    Some(next.unwrap_or_else(|| json!({})).to_string().into_bytes())
                }
                "grim" => pixels.map(|p| p.to_vec()),
                _ => None,
            }
        });
        (run, calls)
    }

    fn window(address: &str, title: &str) -> Value {
        json!({
            "address": address, "title": title, "class": "chromium", "pid": 77,
            "at": [10, 20], "size": [800, 600], "fullscreen": 0, "hidden": false
        })
    }

    #[tokio::test]
    async fn screen_sensor_emits_initial_state_then_only_changes() {
        let (run, calls) = scripted(
            vec![
                Some(window("0xa", "Docs")),
                Some(window("0xa", "Docs")),
                Some(window("0xb", "Mail")),
            ],
            Some(b"P6 pixels"),
        );
        let mut sensor = HyprlandScreenSensor::new(run);

        let first = sensor.poll().await;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].topic, Topic::Screen);
        let payload = &first[0].payload;
        assert_eq!(payload["title"], "Docs");
        assert_eq!(payload["process"], "chromium");
        assert_eq!(payload["pid"], 77);
        assert_eq!(payload["bounds"]["width"], 800);
        assert_eq!(payload["state"], "normal");
        assert_eq!(payload["change"], "focus");
        assert_eq!(payload["content_hash"], fnv1a64(b"P6 pixels"));

        assert!(sensor.poll().await.is_empty(), "unchanged screen is silent");
        let switched = sensor.poll().await;
        assert_eq!(switched[0].payload["title"], "Mail");
        assert!(calls
            .lock()
            .unwrap()
            .contains(&"grim -s 0.25 -t ppm -g 10,20 800x600 -".to_string()));
    }

    #[tokio::test]
    async fn screen_sensor_without_pixels_never_runs_grim() {
        let grim_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = grim_ran.clone();
        let run: CommandFn = Arc::new(move |program: &str, _args: &[&str]| match program {
            "hyprctl" => Some(
                br#"{"address":"0x1","class":"kitty","title":"t","at":[0,0],"size":[100,100]}"#
                    .to_vec(),
            ),
            "grim" => {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Some(b"P6 pixels".to_vec())
            }
            _ => None,
        });
        let mut sensor = HyprlandScreenSensor::new(run).without_pixels();
        let events = sensor.poll().await;
        assert_eq!(events.len(), 1, "identity and geometry are still observed");
        assert!(events[0].payload["content_hash"].is_null());
        assert!(!grim_ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn screen_sensor_degrades_to_no_hash_and_is_silent_without_a_session() {
        let (run, _) = scripted(vec![Some(window("0xa", "Docs"))], None);
        let mut sensor = HyprlandScreenSensor::new(run);
        let events = sensor.poll().await;
        assert!(events[0].payload["content_hash"].is_null());

        let no_session: CommandFn = Arc::new(|_: &str, _: &[&str]| None);
        let mut sensor = HyprlandScreenSensor::new(no_session);
        assert!(
            sensor.poll().await.is_empty(),
            "no Hyprland → nothing invented"
        );
    }

    #[tokio::test]
    async fn focus_sensor_reports_switches_including_to_nothing() {
        let (run, _) = scripted(
            vec![
                Some(window("0xa", "Docs")),
                Some(window("0xa", "Docs")),
                None,
                Some(window("0xb", "Mail")),
            ],
            None,
        );
        let mut sensor = HyprlandFocusSensor::new(run);
        let first = sensor.poll().await;
        assert_eq!(first[0].topic, Topic::WindowFocusChanged);
        assert_eq!(first[0].payload, json!({ "title": "Docs", "pid": 77 }));
        assert!(sensor.poll().await.is_empty());
        assert_eq!(
            sensor.poll().await[0].payload,
            json!({ "title": "", "pid": 0 })
        );
        assert_eq!(sensor.poll().await[0].payload["title"], "Mail");
    }

    /// `cargo test -p friday-sensors hyprland -- --ignored` inside a session.
    #[tokio::test]
    #[ignore = "needs a live Hyprland session"]
    async fn live_sensors_observe_the_focused_window() {
        let run: CommandFn = Arc::new(|program: &str, args: &[&str]| {
            let out = std::process::Command::new(program)
                .args(args)
                .output()
                .ok()?;
            out.status.success().then_some(out.stdout)
        });
        let screen = HyprlandScreenSensor::new(run.clone()).poll().await;
        assert_eq!(screen.len(), 1, "a focused window must be observed");
        assert!(
            screen[0].payload["content_hash"].is_u64(),
            "pixels must hash"
        );
        let focus = HyprlandFocusSensor::new(run).poll().await;
        assert_eq!(focus[0].payload["title"], screen[0].payload["title"]);
    }

    #[test]
    fn observation_maps_hyprland_state() {
        let mut c = window("0xa", "Docs");
        c["fullscreen"] = json!(2);
        assert_eq!(observation_from(&c, None).state, "maximized");
        c["hidden"] = json!(true);
        assert_eq!(observation_from(&c, None).state, "minimized");
        c["pid"] = json!(-1);
        assert_eq!(observation_from(&c, None).pid, 0);
    }
}
