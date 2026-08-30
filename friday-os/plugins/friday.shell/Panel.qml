import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

import "FridayState.qml" as StateDS

Panel {
  id: root
  moduleName: "friday.shell"
  ipcTarget: "friday.shell"
  manageIpc: false

  // ------------------------------------------------------------- the view

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property color ok: bar ? (bar.ok || Color.green) : Color.green
  readonly property color accent: bar ? bar.accent : Color.accent

  // Shared, defensive read-model over the daemon's state.json.
  StateDS.FridayState {
    id: state
    stateFilePath: root.stateFilePath
    // The state plane confirms a sent goal — clear the command latch.
    onActiveChanged: root.confirmSent()
    onLastChanged: root.confirmSent()
    onPresentChanged: root.confirmSent()
  }
  property string stateFilePath: ""

  // A calm status label: only three stable truths, derived from genuine fields.
  readonly property string barLabel: {
    if (!state.present) return "FRIDAY"
    if (!state.running) return "FRIDAY: " + state.status
    if (state.stopping) return "FRIDAY: stopping"
    if (state.active) return "FRIDAY: working"
    return "FRIDAY"
  }

  // Emphasis: subtle "working" tint while a goal is genuinely in flight; an
  // obvious-but-unobtrusive urgent dot when the daemon is down/unavailable.
  readonly property bool working: state.present && state.running && state.active
  readonly property bool attention: !state.present || !state.running

  function refreshNow() {
    state.load()
    root.confirmSent()
  }

  // -------------------------------------------------------- command plane
  // R5-1-3: the deliberate command surface. The user explicitly requests a
  // goal; we shell out to the audited `friday-ctl` client (which reads the
  // live token from state.json itself). Opening the panel never sends anything
  // — the command only fires on an explicit GO. Duplicate-in-flight is blocked
  // here so a second GO while a goal is genuinely running is a no-op (no
  // duplicate execution). The COMMAND PLANE (shell→ctl→daemon, "accepted")
  // is separate from the STATE PLANE (daemon→state.json→shell, the result).

  readonly property bool canCommand: state.present && state.running
  // A synchronous latch so a GO (button or Enter) cannot double-fire in the
  // window before the daemon's `goal.started` lands in state. Cleared when the
  // state plane confirms the goal (becomes active or reaches terminal), then
  // the `state.active` guard takes over for the rest of the run.
  property bool sending: false
  // The exact text we last sent, cleared when it confirms coming back.
  property string sentGoal: ""

  // The audited CLI the widget invokes to run a goal (manifest `ctlCommand` /
  // `ctlAddr` defaults, overridable per-instance). Guarded: no settings means
  // the stock `friday-ctl` on PATH to the default loopback addr.
  function cfg(name, fallback) {
    var s = root.settings
    if (!s || typeof s !== "object") return fallback
    var v = s[name]
    return (v === undefined || v === null || String(v) === "") ? fallback : String(v)
  }
  readonly property string ctlCommand: root.cfg("ctlCommand", "friday-ctl")
  readonly property string ctlAddr: root.cfg("ctlAddr", "127.0.0.1:18432")

  function ctlGoal(goal) {
    var text = String(goal || "")
    if (text.trim() === "") return "empty"
    if (!root.canCommand) return "unavailable"
    // No duplicate execution: refuse while a goal is genuinely running, or in
    // the tiny window before the daemon confirms our send.
    if (root.sending || state.active) return "blocked"
    // Shell-escape the goal into a single argument for the launch seam.
    var quotedGoal = JSON.stringify(text)
    // Compose the command; only pass an --addr override on a non-default addr.
    var cmdline = root.ctlCommand + " goal " + quotedGoal
    if (root.ctlAddr !== "" && root.ctlAddr !== "127.0.0.1:18432") {
      cmdline += " --addr " + root.ctlAddr
    }
    root.bar.run(cmdline)
    root.sending = true
    root.sentGoal = text
    return "sent"
  }

  // The state plane confirms a go: either the goal became active, or it
  // reached a terminal state. Once confirmed, the latch clears so a *new*
  // intentional goal is allowed again (a fresh GO starts a fresh goal).
  function confirmSent() {
    if (root.sending && (state.active || state.last !== null)) {
      root.sending = false
    }
  }

  // Quiet one-line feedback after a GO; clears itself as the state plane
  // confirms the goal became active/terminal.
  function showCommandResult(r) {
    if (r === "sent") {
      commandNote.text = "goal sent"
    } else if (r === "blocked") {
      commandNote.text = "a goal is already running"
    } else if (r === "unavailable") {
      commandNote.text = "daemon unavailable"
    } else if (r === "empty") {
      commandNote.text = "enter a goal first"
    } else {
      commandNote.text = ""
    }
  }

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): string { state.load(); return "ok" }
    function status(): string { return state.running ? "online" : (state.present ? state.status : "offline") }
    // An explicit, immediate command request from other shell IPC.
    function goal(text: string): string { return root.ctlGoal(text) }
  }

  // --------------------------------------------------------- glanceable bar
  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: state.working ? "●" : (state.attention ? "○" : "●")
    // Just enough to read at a glance; the working tint is quiet, the offline
    // dot turns the button accent so it is visible but not alarming.
    active: !state.attention ? root.working : true
    onPressed: function(buttonCode) { root.toggle() }
    ToolTip.text: root.barLabel
    ToolTip.visible: hovered
  }

  // -------------------------------------------------------------- rich panel
  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    contentWidth: panel.fittedContentWidth(Style.space(420))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onActivateRequested: root.refreshNow()
      onTextKey: function(t) {
        if (t === "r" || t === "R") { root.refreshNow(); return }
        if (t === "s" || t === "S") { root.showCommandResult(root.ctlGoal(goalField.text)); return }
      }
    }

    Column {
      id: column
      anchors.fill: parent
      anchors.margins: Style.space(8)
      spacing: Style.space(7)

      Row {
        spacing: Style.space(6)
        Text {
          text: state.working ? "●" : (state.attention ? "○" : "●")
          color: state.attention ? root.urgent : (state.working ? root.accent : root.ok)
          font.pixelSize: Style.font.sizes.text
          anchors.verticalCenter: parent.verticalCenter
        }
        Text {
          text: "FRIDAY"
          font.pixelSize: Style.font.sizes.panel
          font.bold: true
          color: root.foreground
        }
        Text {
          text: state.present ? state.status : "unavailable"
          font.pixelSize: Style.font.sizes.subtext
          color: state.present ? root.dim : root.urgent
          anchors.verticalCenter: parent.verticalCenter
        }
      }

      // A genuine warning only when the file is missing or the daemon is down;
      // quiet and unobtrusive, never a crash.
      Text {
        visible: !state.present || !state.running
        text: state.present
          ? ("daemon " + state.status)
          : "FRIDAY not running — state file missing"
        font.pixelSize: Style.font.sizes.subtext
        color: root.urgent
        wrapMode: Text.WordWrap
        width: parent.width
      }

      Rectangle { width: parent.width; height: 1; color: root.dim }

      // Current activity — shown only when a goal is genuinely in flight.
      Text {
        visible: state.active && state.goalText !== ""
        text: "Working"
        font.pixelSize: Style.font.sizes.text
        font.bold: true
        color: root.accent
      }
      Text {
        visible: state.active && state.goalText !== ""
        text: state.goalText
        font.pixelSize: Style.font.sizes.text
        color: root.foreground
        wrapMode: Text.WordWrap
        width: parent.width
      }

      // Last outcome — genuine detail, or nothing if none has happened.
      Text {
        visible: state.last !== null
        text: "Last goal"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
      }
      Text {
        visible: state.last !== null
        text: state.lastOutcome
        font.pixelSize: Style.font.sizes.panel
        font.bold: true
        color: root.lastOutcome === "completed" ? root.ok : root.urgent
      }
      Text {
        visible: state.last !== null && state.lastResult !== ""
        text: "Result: " + state.lastResult
        font.pixelSize: Style.font.sizes.subtext
        color: root.foreground
        wrapMode: Text.WordWrap
        width: parent.width
      }
      Text {
        visible: state.last !== null && state.lastError !== ""
        text: "Error: " + state.lastError
        font.pixelSize: Style.font.sizes.subtext
        color: root.urgent
        wrapMode: Text.WordWrap
        width: parent.width
      }
      // Phases/strategy are shown as plain numbers only when the runtime
      // genuinely reported them — never an invented "confidence" meter.
      Row {
        visible: state.last !== null
        spacing: Style.space(6)
        Text {
          visible: state.lastPhases > 0
          text: "phases " + state.lastPhases
          font.pixelSize: Style.font.sizes.subtext
          color: root.dim
        }
        Text {
          visible: state.lastStrategy !== ""
          text: "route " + state.lastStrategy
          font.pixelSize: Style.font.sizes.subtext
          color: root.dim
        }
      }

      Rectangle { width: parent.width; height: 1; color: root.dim }

      // ------------------------------------------------ deliberate command
      // R5-1-3: one explicit user action → one real goal. Enter a goal and
      // press GO (or Enter). Nothing fires merely by opening this panel.
      Text {
        text: "Give FRIDAY a goal"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
      }
      Row {
        width: parent.width
        spacing: Style.space(5)
        TextField {
          id: goalField
          width: parent.width - goButton.width - parent.spacing
          height: Style.space(26)
          placeholderText: "e.g. Research Gemma 4"
          color: root.foreground
          enabled: root.canCommand && !state.active
          onAccepted: {
            var r = root.ctlGoal(text)
            root.showCommandResult(r)
          }
        }
        Button {
          id: goButton
          width: Style.space(52)
          height: goalField.height
          text: state.active ? "…" : "GO"
          enabled: root.canCommand && !state.active && goalField.text.trim() !== ""
          onClicked: root.showCommandResult(root.ctlGoal(goalField.text))
        }
      }
      // Calm feedback for the command surface only; never a crash.
      Text {
        id: commandNote
        visible: text !== ""
        text: ""
        font.pixelSize: Style.font.sizes.subtext
        color: root.accent
        wrapMode: Text.WordWrap
        width: parent.width
      }
      Text {
        visible: !root.canCommand
        text: "daemon required to run a goal"
        font.pixelSize: Style.font.sizes.subtext
        color: root.urgent
      }
      Text {
        visible: root.canCommand && state.active
        text: "a goal is running — new goals are queued only after it ends"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
        wrapMode: Text.WordWrap
        width: parent.width
      }

      Rectangle { width: parent.width; height: 1; color: root.dim }

      Row {
        visible: state.voiceOn
        spacing: Style.space(6)
        Text { text: "voice"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text { text: state.voice; font.pixelSize: Style.font.sizes.subtext; color: root.foreground }
      }
      Row {
        visible: state.cpu >= 0
        spacing: Style.space(6)
        Text { text: "cpu"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text { text: state.cpu.toFixed(1) + "%"; font.pixelSize: Style.font.sizes.subtext; color: root.foreground }
        Text { text: "mem"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text {
          text: (state.memUsedMb > 0 && state.memTotalMb > 0)
            ? state.memUsedMb.toFixed(0) + " / " + state.memTotalMb.toFixed(0) + " MB"
            : "—"
          font.pixelSize: Style.font.sizes.subtext
          color: root.foreground
        }
      }
      Text {
        visible: state.lastCommand
        text: "last: " + String((state.lastCommand || {}).method || "")
          + " — " + String((state.lastCommand || {}).goal || "")
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
        elide: Text.ElideRight
        width: parent.width
      }

      Text {
        text: "r to refresh · s to send goal"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
      }
    }
  }
}
