pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The FRIDAY daemon as the shell sees it: a read-only view of state.json
// (specs/ and services/mobile/src/shell_state.rs) plus the explicit actions a
// person can take. Nothing here talks to FRIDAY on its own; every action is a
// button press or a typed question.
Singleton {
  id: root

  readonly property string stateFile: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
    + "/friday/state.json"

  // The phases a goal moves through, in order (shell_state.rs PHASE_ORDER,
  // without the terminal "idle").
  readonly property var phases: ["observe", "attend", "investigate", "plan", "simulate",
    "execute", "verify", "reflect", "consolidate", "learn"]

  property var parsed: ({})

  function str(v) { return (v === undefined || v === null) ? "" : String(v); }
  function obj(v) { return (v && typeof v === "object") ? v : null; }

  FileView {
    id: file
    path: root.stateFile
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try {
        root.parsed = root.obj(JSON.parse(text())) || {};
      } catch (e) {
        root.parsed = {};
      }
    }
    onLoadFailed: root.parsed = ({})
  }

  // The daemon replaces the file atomically, which can drop the watch, and it
  // may not exist yet when the shell starts first. A re-read covers both: slow
  // when idle, quick while a goal runs so a streaming answer reads smoothly.
  Timer {
    interval: root.working ? 300 : 3000
    running: true
    repeat: true
    onTriggered: file.reload()
  }

  // FRIDAY is speaking while `friday-ctl say` holds its pid file
  // ($XDG_RUNTIME_DIR/friday-say.pid, removed when it finishes).
  property bool speaking: false
  FileView {
    id: sayFile
    path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/friday-say.pid"
    printErrors: false
    onLoaded: root.speaking = text().trim() !== ""
    onLoadFailed: root.speaking = false
  }
  Timer {
    interval: 500
    running: true
    repeat: true
    onTriggered: sayFile.reload()
  }

  readonly property string status: str(parsed.status)
  readonly property bool present: status !== ""
  readonly property bool running: status === "running"

  readonly property var current: obj(parsed.current)
  readonly property bool working: running && current !== null && current.active === true
  readonly property string goal: current ? str(current.goal) : ""
  readonly property string phase: current ? str(current.phase) : ""
  // The answer so far while it streams (state.json current.partial).
  readonly property string partial: working ? str(current.partial) : ""
  readonly property int phaseIndex: phases.indexOf(phase)

  readonly property var last: obj(parsed.last_goal)
  readonly property string lastGoal: last ? str(last.goal) : ""
  readonly property string lastOutcome: last ? str(last.outcome) : ""
  readonly property string lastDetail: last ? str(last.result || last.error || last.failed_reason) : ""
  readonly property bool lastFailed: /fail|error|abort|cancel/i.test(lastOutcome)

  // One word for the orb and the bar: offline | stopped | working | ready.
  readonly property string mode: !present ? "offline"
    : !running ? "stopped"
    : working ? "working"
    : "ready"

  readonly property string headline: mode === "offline" ? "FRIDAY is offline"
    : mode === "stopped" ? "FRIDAY is " + (status || "stopped")
    : mode === "working" ? (phase ? "Working · " + phase : "Working")
    : "Ready"

  function ask(question) {
    const q = String(question || "").trim();
    Quickshell.execDetached(q ? ["friday", "ask", q] : ["friday", "ask"]);
  }
  function askSelection() { Quickshell.execDetached(["friday", "ask", "--selection"]); }
  function askScreen() { Quickshell.execDetached(["friday", "ask", "--screen"]); }
  function cancel() { Quickshell.execDetached(["friday", "cancel"]); }
  function startDaemon() { Quickshell.execDetached(["systemctl", "--user", "restart", "friday-linux-daemon"]); }
  function doctor() { Quickshell.execDetached(["foot", "-H", "-T", "FRIDAY doctor", "friday", "doctor"]); }
  function answers() { Quickshell.execDetached(["foot", "-H", "-T", "FRIDAY answers", "friday", "answers"]); }
  function copy(text) { Quickshell.execDetached(["wl-copy", "--", String(text)]); }
}
