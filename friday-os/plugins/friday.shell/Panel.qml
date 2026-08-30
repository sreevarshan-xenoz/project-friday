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

  function refreshNow() { state.load() }

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): string { state.load(); return "ok" }
    function status(): string { return state.running ? "online" : (state.present ? state.status : "offline") }
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
      onTextKey: function(t) { if (t === "r" || t === "R") root.refreshNow() }
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
        text: "r to refresh"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
      }
    }
  }
}
