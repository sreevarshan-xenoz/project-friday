//! Windows Qualification Corpus (Sprint 001A).
//!
//! Each [`Scenario`] is a *fact* we extract from the real `windows-host`: execute
//! an action over IPC, then verify belief against reality (State Accuracy). No
//! recovery, no Desktop State Model — just measurements.

use serde_json::json;

/// A single IPC intent.
#[derive(Debug, Clone)]
pub struct Intent {
    pub intent: &'static str,
    pub params: serde_json::Value,
}

/// What the harness checks after `action` to decide if the fact holds.
#[derive(Debug, Clone)]
pub enum Verify {
    /// Opened window appears in `list` and is foreground.
    Launch,
    /// `foreground` after `action` equals the opened window id.
    Focus,
    /// Bounds in `list` match the expected rectangle.
    Bounds {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// Window must be absent from `list` after the action (close).
    Closed,
    /// Action success is the only observable fact (input ops).
    InputOk,
    /// Clipboard write followed by read must round-trip.
    Clipboard { text: String },
    /// Capture output must contain image data.
    Capture,
    /// `list` must contain the opened window.
    Listed,
    /// `list` must contain a window whose title matches.
    FindByTitle { title: String },
    /// After injection the action must fail *gracefully* (no silent success on a
    /// dead window). A success here is the bug.
    ErrorExpected,
}

/// Failure injection applied between setup and action.
#[derive(Debug, Clone, Copy)]
pub enum Injection {
    /// Kill the launched process (process crash / window disappears).
    KillProcess,
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: &'static str,
    pub group: &'static str,
    /// Optional action that produces a window we act on (e.g. open).
    pub setup: Option<Intent>,
    /// The action under test.
    pub action: Intent,
    pub verify: Verify,
    pub injection: Option<Injection>,
}

fn open(title: &'static str) -> Intent {
    Intent {
        intent: "windows.open",
        params: json!({ "path": "notepad.exe", "title": title }),
    }
}

/// The 001A corpus. Grounded in real `windows-host` intents. One injection
/// (`process_crash`) is included to produce the first benchmark-linked issue and
/// exercise the stale-HWND gap.
pub fn all() -> Vec<Scenario> {
    vec![
        Scenario {
            id: "launch.notepad",
            group: "launch",
            setup: None,
            action: open("Qual-Launch"),
            verify: Verify::Launch,
            injection: None,
        },
        Scenario {
            id: "focus.notepad",
            group: "focus",
            setup: Some(open("Qual-Focus")),
            action: Intent {
                intent: "windows.focus",
                params: json!({}),
            },
            verify: Verify::Focus,
            injection: None,
        },
        Scenario {
            id: "move.notepad",
            group: "geometry",
            setup: Some(open("Qual-Move")),
            action: Intent {
                intent: "windows.move",
                params: json!({ "x": 100, "y": 120 }),
            },
            verify: Verify::Bounds {
                x: 100,
                y: 120,
                width: 800,
                height: 600,
            },
            injection: None,
        },
        Scenario {
            id: "resize.notepad",
            group: "geometry",
            setup: Some(open("Qual-Resize")),
            action: Intent {
                intent: "windows.resize",
                params: json!({ "width": 1024, "height": 768 }),
            },
            verify: Verify::Bounds {
                x: 0,
                y: 0,
                width: 1024,
                height: 768,
            },
            injection: None,
        },
        Scenario {
            id: "close.notepad",
            group: "lifecycle",
            setup: Some(open("Qual-Close")),
            action: Intent {
                intent: "windows.close",
                params: json!({}),
            },
            verify: Verify::Closed,
            injection: None,
        },
        Scenario {
            id: "input.type",
            group: "input",
            setup: Some(open("Qual-Type")),
            action: Intent {
                intent: "windows.type",
                params: json!({ "text": "hello friday" }),
            },
            verify: Verify::InputOk,
            injection: None,
        },
        Scenario {
            id: "input.click",
            group: "input",
            setup: Some(open("Qual-Click")),
            action: Intent {
                intent: "windows.click",
                params: json!({}),
            },
            verify: Verify::InputOk,
            injection: None,
        },
        Scenario {
            id: "input.scroll",
            group: "input",
            setup: Some(open("Qual-Scroll")),
            action: Intent {
                intent: "windows.scroll",
                params: json!({ "delta": 3 }),
            },
            verify: Verify::InputOk,
            injection: None,
        },
        Scenario {
            id: "clipboard.roundtrip",
            group: "clipboard",
            setup: None,
            action: Intent {
                intent: "windows.clipboard.write",
                params: json!({ "text": "qual-clip" }),
            },
            verify: Verify::Clipboard {
                text: "qual-clip".into(),
            },
            injection: None,
        },
        Scenario {
            id: "capture.notepad",
            group: "capture",
            setup: Some(open("Qual-Capture")),
            action: Intent {
                intent: "windows.capture",
                params: json!({}),
            },
            verify: Verify::Capture,
            injection: None,
        },
        Scenario {
            id: "discovery.list",
            group: "discovery",
            setup: Some(open("Qual-List")),
            action: Intent {
                intent: "windows.list",
                params: json!({}),
            },
            verify: Verify::Listed,
            injection: None,
        },
        Scenario {
            id: "discovery.find_by_title",
            group: "discovery",
            setup: Some(open("Qual-FindByTitle")),
            action: Intent {
                intent: "windows.list",
                params: json!({}),
            },
            verify: Verify::FindByTitle {
                title: "Qual-FindByTitle".into(),
            },
            injection: None,
        },
        Scenario {
            id: "injection.process_crash",
            group: "failure-injection",
            setup: Some(open("Qual-Crash")),
            action: Intent {
                intent: "windows.focus",
                params: json!({}),
            },
            verify: Verify::ErrorExpected,
            injection: Some(Injection::KillProcess),
        },
    ]
}
