//! The FRIDAY OS Context Collector (R5-2-5-1).
//!
//! A **pure OS capability**: given a [`ContextChannel`], it produces the
//! channel's [`CapturedContext`] from the OS — nothing more.
//!
//! ```text
//! ContextChannel ──▶ ContextCollector ──▶ CapturedContext
//! ```
//!
//! It does **not** decide whether context may leave the machine. There is no
//! `allow`, `consent`, `provider`, `privacy tier`, or `egress` concept anywhere
//! in this crate's API. That is the frozen R5-2-4 contract:
//!
//! > ContextGate decides whether FRIDAY may disclose a category of context.
//! > EgressGate decides whether the resulting payload is safe to leave.
//! > The collector only says "here is the context the OS can provide."
//!
//! V1 supports four channels: `Screen`, `Selection`, `App`, `Clipboard`.
//! `Files` and `Mic` return [`CaptureError::Unavailable`] until a later slice.
//!
//! On a Windows dev host the collector runs against a
//! [`FakeCommandRunner`](crate::command::FakeCommandRunner); the real
//! [`RealCommandRunner`](crate::command::RealCommandRunner) is qualified on the
//! Omarchy target in R5-2-5-4.

use friday_intelligence::ContextChannel;

use crate::command::{CommandOutput, CommandRunner, RunnerError};

/// A region of the screen in logical coordinates (slurp/Hyprland geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenRegion {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Slurp geometry format ("WxH+X+Y" / "X,Y WxH") used by `grim -g`.
    pub fn slurp(&self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

/// Channel-scoped capture options. The caller (chooser / shell seam) chooses
/// what to capture; the collector never invents scope (no ambient capture).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureOptions {
    /// Region to capture. Required for `Screen` and `Selection`; ignored by
    /// `App` and `Clipboard`.
    pub region: Option<ScreenRegion>,
    /// `Screen` only: capture the whole focused monitor when `true` (defaults
    /// to `false`, which means a `region` must be supplied).
    pub fullscreen: bool,
    /// `Screen` only: capture just the focused window, falling back to the
    /// whole screen when there is none (or it can't be found). Takes
    /// precedence over `region` and `fullscreen`.
    pub focused_window: bool,
}

/// The raw OS-provided payload for one channel.
///
/// Deliberately carries **no** consent/provider semantics — only what the OS
/// can provide the collector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedContext {
    /// An image of the screen / region (PNG bytes as produced by `grim`).
    Image {
        bytes: Vec<u8>,
        mime: &'static str,
        geometry: Option<ScreenRegion>,
    },
    /// Text extracted or read from the OS.
    Text { text: String, source: TextSource },
    /// Metadata about the currently active / focused application window.
    AppInfo {
        class: String,
        title: String,
        geometry: Option<ScreenRegion>,
        focused: bool,
    },
}

/// Where captured text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSource {
    /// OCR of a screen region (`omarchy-capture-text`, grim + tesseract).
    OcrRegion,
    /// The OS clipboard (`wl-paste`).
    Clipboard,
}

/// Why [`ContextCollector::capture`] could not produce a [`CapturedContext`].
///
/// These are **capture** failures only — never consent, authorization, or
/// egress verdicts (frozen R5-2-5-1-0 interface contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// The underlying OS command could not run or exited nonzero.
    Command {
        channel: ContextChannel,
        message: String,
    },
    /// This channel is not capturable (yet). V1: `Files` and `Mic`.
    Unavailable {
        channel: ContextChannel,
        reason: &'static str,
    },
    /// The capture command exceeded its time budget.
    Timeout {
        channel: ContextChannel,
        message: String,
    },
    /// The caller supplied options this channel cannot use (e.g. a regionless
    /// screen capture is impossible).
    InvalidOptions {
        channel: ContextChannel,
        message: String,
    },
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::Command { channel, message } => write!(
                f,
                "capture command failed for channel '{}': {message}",
                channel.label()
            ),
            CaptureError::Unavailable { channel, reason } => write!(
                f,
                "channel '{}' is not available for capture: {reason}",
                channel.label()
            ),
            CaptureError::Timeout { channel, message } => write!(
                f,
                "capture command timed out for channel '{}': {message}",
                channel.label()
            ),
            CaptureError::InvalidOptions { channel, message } => write!(
                f,
                "invalid capture options for channel '{}': {message}",
                channel.label()
            ),
        }
    }
}

/// The pure OS capture capability (R5-2-5-1). Thread-safe against any
/// [`CommandRunner`].
#[derive(Debug, Clone)]
pub struct ContextCollector<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> ContextCollector<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Produce the OS context for `channel`, bounded by `options`.
    ///
    /// - Captures **only** the requested channel, never ambiently.
    /// - Returns `CapturedContext` describing what the OS can provide.
    /// - `Files` and `Mic` return `Unavailable` (V1 scope).
    pub fn capture(
        &self,
        channel: ContextChannel,
        options: &CaptureOptions,
    ) -> Result<CapturedContext, CaptureError> {
        match channel {
            ContextChannel::Screen => self.capture_screen(options),
            ContextChannel::Selection => self.capture_selection(options),
            ContextChannel::App => self.capture_app(),
            ContextChannel::Clipboard => self.capture_clipboard(),
            ContextChannel::Files => Err(CaptureError::Unavailable {
                channel,
                reason: "file context capture deferred to a later R5-2 slice",
            }),
            ContextChannel::Mic => Err(CaptureError::Unavailable {
                channel,
                reason: "microphone context capture deferred to a later R5-2 slice",
            }),
        }
    }

    fn capture_screen(&self, options: &CaptureOptions) -> Result<CapturedContext, CaptureError> {
        let channel = ContextChannel::Screen;
        let geometry = if options.focused_window {
            self.focused_window()
        } else if options.fullscreen {
            None
        } else {
            Some(options.region.ok_or(CaptureError::InvalidOptions {
                channel,
                message: "screen capture requires a region or fullscreen=true".to_string(),
            })?)
        };
        let (program, args) = screen_command(geometry);
        let out = self.run_capture(channel, &program, &args)?;

        Ok(CapturedContext::Image {
            bytes: out.stdout,
            mime: "image/png",
            geometry,
        })
    }

    fn capture_selection(&self, options: &CaptureOptions) -> Result<CapturedContext, CaptureError> {
        let channel = ContextChannel::Selection;
        let region = options.region.ok_or(CaptureError::InvalidOptions {
            channel,
            message: "selection capture requires a region".to_string(),
        })?;

        let args = vec!["--stdout".to_string(), region.slurp()];
        let out = self.run_capture(channel, "omarchy-capture-text", &args)?;
        let text = String::from_utf8(out.stdout)
            .map_err(|e| CaptureError::Command {
                channel,
                message: format!("selection output was not UTF-8: {e}"),
            })?
            .trim()
            .to_string();

        Ok(CapturedContext::Text {
            text,
            source: TextSource::OcrRegion,
        })
    }

    fn capture_app(&self) -> Result<CapturedContext, CaptureError> {
        let channel = ContextChannel::App;
        let (program, args) = window_command();
        let out = match self.run_capture(channel, &program, &args) {
            Ok(out) => out,
            // No hyprctl: niri's focused window, in its own shape (`app_id`).
            Err(e) if !cfg!(windows) => {
                let niri = ["msg", "--json", "focused-window"].map(String::from);
                self.run_capture(channel, "niri", &niri)
                    .map_err(|_| CaptureError::Command {
                        channel,
                        message: e.to_string(),
                    })?
            }
            Err(e) => {
                return Err(CaptureError::Command {
                    channel,
                    message: e.to_string(),
                })
            }
        };
        parse_activewindow(&out.stdout)
            .map_err(|message| CaptureError::Command { channel, message })
    }

    /// The focused window's rectangle, or `None` when there is no focused
    /// window, the lookup fails, or it has no size (a screen ask then takes
    /// the whole screen rather than failing).
    pub fn focused_window(&self) -> Option<ScreenRegion> {
        match self.capture_app() {
            Ok(CapturedContext::AppInfo {
                geometry: Some(r), ..
            }) if r.width > 0 && r.height > 0 => Some(r),
            _ if !cfg!(windows) => self.niri_focused_window(),
            _ => None,
        }
    }

    /// The focused window on niri, which has no hyprctl (see
    /// [`niri_window_region`]). `None` when niri isn't running or nothing is
    /// focused.
    fn niri_focused_window(&self) -> Option<ScreenRegion> {
        let json = |what: &str| -> Option<serde_json::Value> {
            let args = ["msg", "--json", what].map(String::from);
            let out = self.runner.run("niri", &args).ok()?;
            if out.status.is_some_and(|code| code != 0) {
                return None;
            }
            serde_json::from_slice(&out.stdout).ok()
        };
        niri_window_region(
            &json("focused-window")?,
            &json("workspaces")?,
            &json("outputs")?,
        )
    }

    fn capture_clipboard(&self) -> Result<CapturedContext, CaptureError> {
        let channel = ContextChannel::Clipboard;
        let args = vec!["--no-newline".to_string()];
        let out = self.run_capture(channel, "wl-paste", &args)?;
        let text = String::from_utf8(out.stdout).map_err(|e| CaptureError::Command {
            channel,
            message: format!("clipboard output was not UTF-8: {e}"),
        })?;
        Ok(CapturedContext::Text {
            text,
            source: TextSource::Clipboard,
        })
    }

    fn run_capture(
        &self,
        channel: ContextChannel,
        program: &str,
        args: &[String],
    ) -> Result<CommandOutput, CaptureError> {
        let out = self.runner.run(program, args).map_err(|e| match e {
            RunnerError::Io(message) => CaptureError::Command { channel, message },
            RunnerError::Timeout(message) => CaptureError::Timeout { channel, message },
        })?;

        // Nonzero exit (but clean spawn) is a command failure, not a payload.
        if let Some(code) = out.status {
            if code != 0 {
                return Err(CaptureError::Command {
                    channel,
                    message: format!(
                        "{program} exited {code}: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                });
            }
        }
        Ok(out)
    }
}

/// The program and arguments that write a PNG of the screen (or `region`) to
/// stdout: `grim` on Wayland, and on Windows FRIDAY's own host
/// (`friday-windows-host --screenshot [x y w h]`), which has no grim.
pub fn screen_capture_command(
    region: Option<ScreenRegion>,
    windows: bool,
    windows_host: &str,
) -> (String, Vec<String>) {
    if windows {
        let mut args = vec!["--screenshot".to_string()];
        if let Some(r) = region {
            args.extend([
                r.x.to_string(),
                r.y.to_string(),
                r.width.to_string(),
                r.height.to_string(),
            ]);
        }
        (windows_host.to_string(), args)
    } else {
        let args = match region {
            Some(r) => vec!["-g".to_string(), r.slurp(), "-".to_string()],
            None => vec!["-".to_string()],
        };
        ("grim".to_string(), args)
    }
}

/// The program and arguments that print the focused window as JSON
/// (`{"at": [x, y], "size": [w, h], "class": .., "title": ..}`):
/// `hyprctl -j activewindow` on Hyprland, and on Windows FRIDAY's host
/// (`friday-windows-host --active-window`), in the same shape.
pub fn active_window_command(windows: bool, windows_host: &str) -> (String, Vec<String>) {
    if windows {
        (
            windows_host.to_string(),
            vec!["--active-window".to_string()],
        )
    } else {
        (
            "hyprctl".to_string(),
            vec!["-j".to_string(), "activewindow".to_string()],
        )
    }
}

/// This platform's focused-window command (see [`active_window_command`]).
pub fn window_command() -> (String, Vec<String>) {
    active_window_command(cfg!(windows), &windows_host_program())
}

/// This platform's screen capture command (see [`screen_capture_command`]);
/// tests script their fake capture with it so they hold on every platform.
pub fn screen_command(region: Option<ScreenRegion>) -> (String, Vec<String>) {
    screen_capture_command(region, cfg!(windows), &windows_host_program())
}

/// `friday-windows-host.exe` next to the running daemon (the installer puts
/// them together), else the bare name for PATH lookup.
pub fn windows_host_program() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("friday-windows-host.exe")))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "friday-windows-host".to_string())
}

/// Parse `hyprctl -j activewindow` JSON into [`CapturedContext::AppInfo`].
fn parse_activewindow(stdout: &[u8]) -> Result<CapturedContext, String> {
    let v: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|e| format!("invalid hyprctl JSON: {e}"))?;

    let class = field(&v, "class")
        .or_else(|| field(&v, "initialClass"))
        .or_else(|| field(&v, "app_id"));
    let title = field(&v, "title").or_else(|| field(&v, "initialTitle"));
    let geometry = rect(&v, "at", "size");

    Ok(CapturedContext::AppInfo {
        class: class.unwrap_or_default(),
        title: title.unwrap_or_default(),
        geometry,
        focused: true,
    })
}

/// A niri window's rectangle in global logical coordinates, as `grim -g`
/// takes it: niri reports the tile's position in its workspace view and the
/// window's offset in the tile, both relative to the output the workspace is
/// on, so the output's logical position is added. `None` for a window that
/// isn't in view or has no size.
fn niri_window_region(
    window: &serde_json::Value,
    workspaces: &serde_json::Value,
    outputs: &serde_json::Value,
) -> Option<ScreenRegion> {
    let pair = |v: &serde_json::Value, key: &str| -> Option<(f64, f64)> {
        let a = v.get(key)?.as_array()?;
        Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let layout = window.get("layout")?;
    let (tile_x, tile_y) = pair(layout, "tile_pos_in_workspace_view")?;
    let (off_x, off_y) = pair(layout, "window_offset_in_tile")?;
    let (width, height) = pair(layout, "window_size")?;
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let workspace_id = window.get("workspace_id")?.as_u64()?;
    let output = workspaces
        .as_array()?
        .iter()
        .find(|w| w.get("id").and_then(|id| id.as_u64()) == Some(workspace_id))?
        .get("output")?
        .as_str()?;
    let logical = outputs.get(output)?.get("logical")?;
    let (out_x, out_y) = (logical.get("x")?.as_f64()?, logical.get("y")?.as_f64()?);
    Some(ScreenRegion::new(
        (out_x + tile_x + off_x).round() as i32,
        (out_y + tile_y + off_y).round() as i32,
        width as u32,
        height as u32,
    ))
}

fn field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|f| f.as_str()).map(|s| s.to_string())
}

fn rect(v: &serde_json::Value, pos_key: &str, size_key: &str) -> Option<ScreenRegion> {
    let pos = v.get(pos_key)?.as_array()?;
    let size = v.get(size_key)?.as_array()?;
    Some(ScreenRegion::new(
        pos.first()?.as_i64()? as i32,
        pos.get(1)?.as_i64()? as i32,
        size.first()?.as_u64()? as u32,
        size.get(1)?.as_u64()? as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::FakeCommandRunner;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-bytes";
    const EMPTY: CaptureOptions = CaptureOptions {
        region: None,
        fullscreen: false,
        focused_window: false,
    };

    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    fn collector() -> ContextCollector<FakeCommandRunner> {
        let (region_prog, region_args) = screen_command(Some(ScreenRegion::new(10, 20, 640, 480)));
        let (full_prog, full_args) = screen_command(None);
        let runner = FakeCommandRunner::new()
            .script_stdout(&region_prog, &strs(&region_args), PNG)
            .script_stdout(&full_prog, &strs(&full_args), PNG)
            .script_stdout(
                "omarchy-capture-text",
                &["--stdout", "10,20 640x480"],
                "Quarterly revenue up 12%\n",
            )
            .script_stdout(
                &window_command().0,
                &strs(&window_command().1),
                br#"{"class":"kitty","title":"project-friday","at":[0,0],"size":[1920,1080]}"#,
            )
            .script_stdout("wl-paste", &["--no-newline"], "https://example.com/docs");
        ContextCollector::new(runner)
    }

    fn screen_region() -> CaptureOptions {
        CaptureOptions {
            region: Some(ScreenRegion::new(10, 20, 640, 480)),
            fullscreen: false,
            focused_window: false,
        }
    }

    #[test]
    fn screen_region_captures_png_with_geometry() {
        let ctx = collector()
            .capture(ContextChannel::Screen, &screen_region())
            .unwrap();
        assert_eq!(
            ctx,
            CapturedContext::Image {
                bytes: PNG.to_vec(),
                mime: "image/png",
                geometry: Some(ScreenRegion::new(10, 20, 640, 480)),
            }
        );
    }

    #[test]
    fn screen_fullscreen_needs_no_region() {
        let opts = CaptureOptions {
            region: None,
            fullscreen: true,
            focused_window: false,
        };
        let ctx = collector().capture(ContextChannel::Screen, &opts).unwrap();
        assert_eq!(
            ctx,
            CapturedContext::Image {
                bytes: PNG.to_vec(),
                mime: "image/png",
                geometry: None
            }
        );
    }

    fn focused(window_json: Option<&[u8]>) -> ContextCollector<FakeCommandRunner> {
        let region = ScreenRegion::new(100, 50, 800, 600);
        let (rp, ra) = screen_command(Some(region));
        let (fp, fa) = screen_command(None);
        let mut runner = FakeCommandRunner::new()
            .script_stdout(&rp, &strs(&ra), PNG)
            .script_stdout(&fp, &strs(&fa), PNG);
        if let Some(json) = window_json {
            let (wp, wa) = window_command();
            runner = runner.script_stdout(&wp, &strs(&wa), json);
        }
        ContextCollector::new(runner)
    }

    fn focused_window_options() -> CaptureOptions {
        CaptureOptions {
            focused_window: true,
            ..EMPTY
        }
    }

    #[test]
    fn screen_takes_just_the_focused_window() {
        let c = focused(Some(
            br#"{"class":"code","title":"main.rs","at":[100,50],"size":[800,600]}"#,
        ));
        match c
            .capture(ContextChannel::Screen, &focused_window_options())
            .unwrap()
        {
            CapturedContext::Image { geometry, .. } => {
                assert_eq!(geometry, Some(ScreenRegion::new(100, 50, 800, 600)))
            }
            other => panic!("expected an image, got {other:?}"),
        }
    }

    #[test]
    fn screen_falls_back_to_the_whole_screen_without_a_focused_window() {
        // The lookup fails (no compositor answer, or not scripted at all) ...
        for c in [focused(None), focused(Some(b"not json"))] {
            match c
                .capture(ContextChannel::Screen, &focused_window_options())
                .unwrap()
            {
                CapturedContext::Image { geometry, .. } => assert_eq!(geometry, None),
                other => panic!("expected an image, got {other:?}"),
            }
        }
        // ... or the focused "window" has no size (an empty workspace).
        let c = focused(Some(br#"{"at":[0,0],"size":[0,0]}"#));
        match c
            .capture(ContextChannel::Screen, &focused_window_options())
            .unwrap()
        {
            CapturedContext::Image { geometry, .. } => assert_eq!(geometry, None),
            other => panic!("expected an image, got {other:?}"),
        }
    }

    /// niri has no hyprctl: the window's rectangle comes from niri's IPC —
    /// its tile in the workspace view plus its offset in the tile, moved onto
    /// the output the workspace is on.
    #[cfg(not(windows))]
    const NIRI_WINDOW: &[u8] = br#"{"id":7,"title":"main.rs","app_id":"code","workspace_id":3,
        "layout":{"pos_in_scrolling_layout":[1,1],"tile_size":[804.0,604.0],"window_size":[800,600],
        "tile_pos_in_workspace_view":[98.0,48.0],"window_offset_in_tile":[2.0,2.0]}}"#;
    #[cfg(not(windows))]
    const NIRI_WORKSPACES: &[u8] =
        br#"[{"id":1,"idx":1,"output":"HDMI-A-1"},{"id":3,"idx":1,"output":"DP-1"}]"#;
    #[cfg(not(windows))]
    const NIRI_OUTPUTS: &[u8] = br#"{"DP-1":{"name":"DP-1","logical":{"x":1920,"y":0,"width":2560,"height":1440,"scale":1.0}},
        "HDMI-A-1":{"name":"HDMI-A-1","logical":{"x":0,"y":0,"width":1920,"height":1080,"scale":1.0}}}"#;

    #[cfg(not(windows))]
    fn on_niri(window: &[u8]) -> ContextCollector<FakeCommandRunner> {
        let region = ScreenRegion::new(2020, 50, 800, 600);
        let (rp, ra) = screen_command(Some(region));
        let (fp, fa) = screen_command(None);
        let (wp, wa) = window_command();
        let runner = FakeCommandRunner::new()
            .script_stdout(&rp, &strs(&ra), PNG)
            .script_stdout(&fp, &strs(&fa), PNG)
            .script_error(&wp, &strs(&wa), "HYPRLAND_INSTANCE_SIGNATURE not set", 1)
            .script_stdout("niri", &["msg", "--json", "focused-window"], window)
            .script_stdout("niri", &["msg", "--json", "workspaces"], NIRI_WORKSPACES)
            .script_stdout("niri", &["msg", "--json", "outputs"], NIRI_OUTPUTS);
        ContextCollector::new(runner)
    }

    #[cfg(not(windows))]
    #[test]
    fn screen_takes_just_the_focused_window_on_niri() {
        match on_niri(NIRI_WINDOW)
            .capture(ContextChannel::Screen, &focused_window_options())
            .unwrap()
        {
            CapturedContext::Image { geometry, .. } => {
                assert_eq!(geometry, Some(ScreenRegion::new(2020, 50, 800, 600)))
            }
            other => panic!("expected an image, got {other:?}"),
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn the_app_in_front_on_niri() {
        match on_niri(br#"{"id":7,"title":"Inbox - Mail","app_id":"thunderbird"}"#)
            .capture(ContextChannel::App, &CaptureOptions::default())
            .unwrap()
        {
            CapturedContext::AppInfo { class, title, .. } => {
                assert_eq!(
                    (class.as_str(), title.as_str()),
                    ("thunderbird", "Inbox - Mail")
                )
            }
            other => panic!("expected app info, got {other:?}"),
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn niri_without_a_focused_window_takes_the_whole_screen() {
        for window in [&b"null"[..], br#"{"id":7,"workspace_id":3,"layout":{"window_size":[800,600],"window_offset_in_tile":[0.0,0.0],"tile_pos_in_workspace_view":null}}"#] {
            match on_niri(window)
                .capture(ContextChannel::Screen, &focused_window_options())
                .unwrap()
            {
                CapturedContext::Image { geometry, .. } => assert_eq!(geometry, None),
                other => panic!("expected an image, got {other:?}"),
            }
        }
    }

    #[test]
    fn active_window_command_per_platform() {
        let (p, a) = active_window_command(false, "host.exe");
        assert_eq!(
            (p.as_str(), strs(&a)),
            ("hyprctl", vec!["-j", "activewindow"])
        );
        let (p, a) = active_window_command(true, "host.exe");
        assert_eq!(
            (p.as_str(), strs(&a)),
            ("host.exe", vec!["--active-window"])
        );
    }

    #[test]
    fn screen_capture_command_per_platform() {
        let r = ScreenRegion::new(10, 20, 300, 200);
        assert_eq!(
            screen_capture_command(None, false, "host"),
            ("grim".to_string(), vec!["-".to_string()])
        );
        assert_eq!(
            screen_capture_command(Some(r), false, "host").1,
            vec!["-g", "10,20 300x200", "-"]
        );
        // Windows has no grim: FRIDAY's host writes the PNG instead.
        assert_eq!(
            screen_capture_command(None, true, "C:/F/friday-windows-host.exe"),
            (
                "C:/F/friday-windows-host.exe".to_string(),
                vec!["--screenshot".to_string()]
            )
        );
        assert_eq!(
            screen_capture_command(Some(r), true, "h").1,
            vec!["--screenshot", "10", "20", "300", "200"]
        );
    }

    #[test]
    fn screen_without_options_is_invalid() {
        let err = collector()
            .capture(ContextChannel::Screen, &EMPTY)
            .unwrap_err();
        assert!(matches!(
            err,
            CaptureError::InvalidOptions {
                channel: ContextChannel::Screen,
                ..
            }
        ));
    }

    #[test]
    fn selection_ocrs_region_into_text() {
        let ctx = collector()
            .capture(ContextChannel::Selection, &screen_region())
            .unwrap();
        assert_eq!(
            ctx,
            CapturedContext::Text {
                text: "Quarterly revenue up 12%".to_string(),
                source: TextSource::OcrRegion,
            }
        );
    }

    #[test]
    fn selection_without_region_is_invalid() {
        let err = collector()
            .capture(ContextChannel::Selection, &EMPTY)
            .unwrap_err();
        assert!(matches!(
            err,
            CaptureError::InvalidOptions {
                channel: ContextChannel::Selection,
                ..
            }
        ));
    }

    #[test]
    fn app_parses_hyprctl_activewindow_json() {
        let ctx = collector().capture(ContextChannel::App, &EMPTY).unwrap();
        assert_eq!(
            ctx,
            CapturedContext::AppInfo {
                class: "kitty".to_string(),
                title: "project-friday".to_string(),
                geometry: Some(ScreenRegion::new(0, 0, 1920, 1080)),
                focused: true,
            }
        );
    }

    #[test]
    fn app_invalid_json_is_a_capture_error() {
        let runner = FakeCommandRunner::new().script_stdout(
            "hyprctl",
            &["-j", "activewindow"],
            b"{ not json",
        );
        let err = ContextCollector::new(runner)
            .capture(ContextChannel::App, &EMPTY)
            .unwrap_err();
        assert!(matches!(
            err,
            CaptureError::Command {
                channel: ContextChannel::App,
                ..
            }
        ));
    }

    #[test]
    fn clipboard_reads_text() {
        let ctx = collector()
            .capture(ContextChannel::Clipboard, &EMPTY)
            .unwrap();
        assert_eq!(
            ctx,
            CapturedContext::Text {
                text: "https://example.com/docs".to_string(),
                source: TextSource::Clipboard,
            }
        );
    }

    #[test]
    fn files_and_mic_are_unavailable_in_v1() {
        let c = collector();
        let files = c.capture(ContextChannel::Files, &EMPTY).unwrap_err();
        let mic = c.capture(ContextChannel::Mic, &EMPTY).unwrap_err();
        assert!(matches!(
            files,
            CaptureError::Unavailable {
                channel: ContextChannel::Files,
                ..
            }
        ));
        assert!(matches!(
            mic,
            CaptureError::Unavailable {
                channel: ContextChannel::Mic,
                ..
            }
        ));
        assert!(files.to_string().contains("files"));
        assert!(mic.to_string().contains("microphone"));
    }

    #[test]
    fn nonzero_command_exit_is_reported_not_fabricated() {
        let (prog, args) = screen_command(Some(ScreenRegion::new(10, 20, 640, 480)));
        let runner =
            FakeCommandRunner::new().script_error(&prog, &strs(&args), "failed to capture", 1);
        let err = ContextCollector::new(runner)
            .capture(ContextChannel::Screen, &screen_region())
            .unwrap_err();
        assert!(matches!(
            err,
            CaptureError::Command {
                channel: ContextChannel::Screen,
                ..
            }
        ));
        assert!(err.to_string().contains("exited 1"));
    }

    #[test]
    fn unexpected_command_is_loud_failure() {
        // No script for wl-paste: the collector must never silently fall back.
        let runner = FakeCommandRunner::new();
        let err = ContextCollector::new(runner)
            .capture(ContextChannel::Clipboard, &EMPTY)
            .unwrap_err();
        assert!(matches!(
            err,
            CaptureError::Command {
                channel: ContextChannel::Clipboard,
                ..
            }
        ));
        assert!(err.to_string().contains("unscripted"));
    }

    /// The collector's output vocabulary is closed: only Image / Text / AppInfo
    /// payloads exist. There is no allow/consent/provider/privacy variant — a
    /// structural statement that the collector can never become a privacy gate.
    #[test]
    fn captured_context_has_no_privacy_verdict() {
        let img = CapturedContext::Image {
            bytes: vec![],
            mime: "image/png",
            geometry: None,
        };
        let txt = CapturedContext::Text {
            text: String::new(),
            source: TextSource::Clipboard,
        };
        let app = CapturedContext::AppInfo {
            class: String::new(),
            title: String::new(),
            geometry: None,
            focused: true,
        };
        assert!(matches!(img, CapturedContext::Image { .. }));
        assert!(matches!(txt, CapturedContext::Text { .. }));
        assert!(matches!(app, CapturedContext::AppInfo { .. }));
    }
}
