import QtQuick
import Quickshell
import Quickshell.Io

// The read side of the FRIDAY file-backed contract (R5-1-2). The daemon writes
// a single `state.json`; this watches it with FileView (so it reacts to the
// atomic tmp+rename writes the daemon uses) and exposes a parsed, defensive
// view to the panel.
//
// Defensive by design: this plugin runs inside the Omarchy shell host and is
// not sandboxed, so EVERY accessor is guarded. A missing file, malformed JSON,
// an empty/partial document, or a vanished field must degrade to a safe value
// ("FRIDAY unavailable" / stale), never a property error that kills the widget.
Item {
  id: root
  visible: false

  // ------------------------------------------------------------------ path

  readonly property string home: Quickshell.env("HOME") || ""
  readonly property string stateFile: {
    var cfg = String(root.stateFilePath || "")
    if (cfg !== "") return cfg
    var base = Quickshell.env("XDG_STATE_HOME") || home + "/.local/state"
    return base + "/friday/state.json"
  }
  // Optional override from the caller (the panel setting).
  property string stateFilePath: ""

  // The raw parsed object, or {} for missing/malformed. `parsed` NEVER throws.
  property var parsed: ({})

  FileView {
    id: fileView
    paths: [root.stateFile]
    rerunOnFileChange: true
    onExistsChanged: root.load()
    onDataChanged: root.load()
  }

  function load() {
    var text = fileView.getData(root.stateFile)
    if (!text) {
      root.parsed = {}
      root.loadedOnce = true
      return
    }
    var obj
    try {
      obj = JSON.parse(text)
    } catch (e) {
      // Malformed state: keep the last good view fresh and flag parse failure.
      root.parsed = {}
      root.parseError = true
      root.loadedOnce = true
      return
    }
    root.parseError = false
    root.parsed = (obj && typeof obj === "object") ? obj : {}
    root.loadedOnce = true
  }

  Component.onCompleted: root.load()

  property bool loadedOnce: false
  property bool parseError: false

  // Guards --------------------------------------------------------------
  function str(v) { return v === undefined || v === null ? "" : String(v) }
  function num(v) {
    var n = Number(v)
    return isNaN(n) ? 0 : n
  }
  function bool(v) { return v === true }

  // ----------------------------------------------------------------- view

  // `present` is true only if the file exists. Even then every field stays
  // guarded, so a stale/partial document degrades gracefully.
  readonly property bool present: fileView.exists

  readonly property string status: str(root.parsed.status)
  readonly property bool running: root.status === "running"
  readonly property bool stopping: root.status === "stopping"
  readonly property bool starting: root.status === "starting"

  // Current goal (active or just-accepted), guarded.
  readonly property var cur: root.parsed.current && typeof root.parsed.current === "object"
    ? root.parsed.current : null
  readonly property bool active: root.cur !== null && root.bool(root.cur.active)
  readonly property string goalText: root.cur ? root.str(root.cur.goal) : ""
  readonly property string goalPhase: root.cur ? root.str(root.cur.phase) : ""

  // Last goal (terminal), guarded. Genuine outcome detail only.
  readonly property var last: root.parsed.last_goal && typeof root.parsed.last_goal === "object"
    ? root.parsed.last_goal : null
  readonly property string lastOutcome: root.last ? root.str(root.last.outcome) : ""
  readonly property string lastResult: root.last ? root.str(root.last.result) : ""
  readonly property string lastError: root.last ? root.str(root.last.error || root.last.failed_reason) : ""
  readonly property string lastStrategy: root.last ? root.str(root.last.strategy) : ""
  readonly property int lastPhases: root.last ? root.num(root.last.phases) : 0
  readonly property bool lastEvVerified: root.last ? root.last.evidence_verified !== undefined : false

  // Voice + telemetry, guarded.
  readonly property string voice: root.str(root.parsed.voice)
  readonly property bool voiceOn: root.voice !== "" && root.voice !== "off"
  readonly property real cpu: (root.parsed.telemetry && root.parsed.telemetry.cpu_percent !== undefined)
    ? root.num(root.parsed.telemetry.cpu_percent) : -1
  readonly property real memUsedMb: (root.parsed.telemetry && root.parsed.telemetry.memory_used_mb !== undefined)
    ? root.num(root.parsed.telemetry.memory_used_mb) : 0
  readonly property real memTotalMb: (root.parsed.telemetry && root.parsed.telemetry.memory_total_mb !== undefined)
    ? root.num(root.parsed.telemetry.memory_total_mb) : 0

  // The last shell command the daemon accepted (echo only).
  readonly property var lastCommand: root.parsed.command && typeof root.parsed.command === "object"
    ? root.parsed.command : null
}
