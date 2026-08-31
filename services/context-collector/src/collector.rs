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

        let (args, geometry) = if options.fullscreen {
            (vec!["-".to_string()], None)
        } else {
            let region = options.region.ok_or(CaptureError::InvalidOptions {
                channel,
                message: "screen capture requires a region or fullscreen=true".to_string(),
            })?;
            (
                vec!["-g".to_string(), region.slurp(), "-".to_string()],
                Some(region),
            )
        };

        let out = self.run_capture(channel, "grim", &args)?;

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
        let args = vec!["-j".to_string(), "activewindow".to_string()];
        let out =
            self.run_capture(channel, "hyprctl", &args)
                .map_err(|e| CaptureError::Command {
                    channel,
                    message: e.to_string(),
                })?;
        parse_activewindow(&out.stdout)
            .map_err(|message| CaptureError::Command { channel, message })
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

/// Parse `hyprctl -j activewindow` JSON into [`CapturedContext::AppInfo`].
fn parse_activewindow(stdout: &[u8]) -> Result<CapturedContext, String> {
    let v: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|e| format!("invalid hyprctl JSON: {e}"))?;

    let class = field(&v, "class").or_else(|| field(&v, "initialClass"));
    let title = field(&v, "title").or_else(|| field(&v, "initialTitle"));
    let geometry = rect(&v, "at", "size");

    Ok(CapturedContext::AppInfo {
        class: class.unwrap_or_default(),
        title: title.unwrap_or_default(),
        geometry,
        focused: true,
    })
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
    };

    fn collector() -> ContextCollector<FakeCommandRunner> {
        let runner = FakeCommandRunner::new()
            .script_stdout("grim", &["-g", "10,20 640x480", "-"], PNG)
            .script_stdout("grim", &["-"], PNG)
            .script_stdout(
                "omarchy-capture-text",
                &["--stdout", "10,20 640x480"],
                "Quarterly revenue up 12%\n",
            )
            .script_stdout(
                "hyprctl",
                &["-j", "activewindow"],
                br#"{"class":"kitty","title":"project-friday","at":[0,0],"size":[1920,1080]}"#,
            )
            .script_stdout("wl-paste", &["--no-newline"], "https://example.com/docs");
        ContextCollector::new(runner)
    }

    fn screen_region() -> CaptureOptions {
        CaptureOptions {
            region: Some(ScreenRegion::new(10, 20, 640, 480)),
            fullscreen: false,
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
        let runner = FakeCommandRunner::new().script_error(
            "grim",
            &["-g", "10,20 640x480", "-"],
            "failed to capture",
            1,
        );
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
