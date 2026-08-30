import QtQuick
import Quickshell
import Quickshell.Io

// The read side of the FRIDAY file-backed contract. The daemon writes a single
// `state.json` document; this watches it with FileView (so it observes atomic
// tmp+rename writes and reacts to every change) and exposes a parsed view to
// the bar widget. Mirrors the Omarchy `agents` plugin precedent: an external
// CLI writes JSON to state, a widget renders it.
Item {
  id: root
  visible: false

  // Installer copies this plugin to ~/.config/omarchy/plugins/friday.shell/.
  // The state file defaults to ~/.local/state/friday/state.json, the same
  // path `friday-linux-daemon` writes (FRIDAY_STATE_FILE overrides it there).
  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string stateFile: {
    var cfg = String(root.stateFilePath || "")
    if (cfg !== "") return cfg
    var base = Quickshell.env("XDG_STATE_HOME") || home + "/.local/state"
    return base + "/friday/state.json"
  }
  property string stateFilePath: "" // optional override from the caller

  readonly property var state: parsed
  readonly property bool present: fileView.exists && fetched
  property bool fetched: false

  // The raw JSON object, or {} when the file is missing/unparseable.
  property var parsed: ({})

  FileView {
    id: fileView
    paths: [root.stateFile]
    // rerunOnFileChange re-issues a change signal on every write to the file,
    // which the atomic tmp+rename the daemon uses does not always produce.
    rerunOnFileChange: true
    onExistsChanged: if (exists) root.load()
    onDataChanged: root.load()
  }

  function load() {
    fetched = true
    var text = fileView.getData(root.stateFile)
    if (!text) {
      parsed = {}
      return
    }
    try {
      parsed = JSON.parse(text)
    } catch (e) {
      parsed = {}
    }
  }

  Component.onCompleted: if (fileView.exists) root.load()

  // ------------------------------------------------------------------ view

  readonly property string status: String((parsed && parsed.status) || "unknown")
  readonly property string token: String((parsed && parsed.token) || "")
  readonly property bool running: status === "running"
  readonly property bool stopping: status === "stopping"

  readonly property var currentGoal: parsed && parsed.current && parsed.current.goal
    ? parsed.current.goal : ""
  readonly property string currentPhase: parsed && parsed.current
    ? String(parsed.current.phase || "") : ""

  readonly property var lastGoal: parsed && parsed.last_goal ? parsed.last_goal : null
  readonly property string voice: String((parsed && parsed.voice) || "off")
  readonly property real cpu: parsed && parsed.telemetry ? Number(parsed.telemetry.cpu_percent || 0) : -1
  readonly property real memUsedMb: parsed && parsed.telemetry ? Number(parsed.telemetry.memory_used_mb || 0) : 0
  readonly property real memTotalMb: parsed && parsed.telemetry ? Number(parsed.telemetry.memory_total_mb || 1) : 1

  // The last accepted command, for echo in the UI.
  readonly property var lastCommand: parsed && parsed.command ? parsed.command : null

  // True when any cognition slot is populated (the daemon has surfaced state).
  readonly property bool hasCognition: parsed && parsed.cognition
    && Object.keys(parsed.cognition).length > 0
}
