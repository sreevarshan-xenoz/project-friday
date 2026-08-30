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

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property color accent: bar ? bar.accent : Color.accent

  // The shared read-model: a FileView over the daemon's state.json. The same
  // contract `friday-ctl` reads; the panel only ever observes it.
  StateDS.FridayState {
    id: state
    stateFilePath: root.stateFilePath
  }

  property string stateFilePath: ""
  property int refreshIntervalSec: 5

  Timer {
    interval: Math.max(1, root.refreshIntervalSec) * 1000
    running: true
    repeat: true
    onTriggered: state.load()
  }

  // -------------------------------------------------------------- ctl

  function ctlGoal(goal) {
    if (!state.present) {
      root.bar.notify("FRIDAY daemon is not running")
      return false
    }
    var args = "goal " + JSON.stringify(String(goal || ""))
    if (root.ctlCommand !== "") {
      var base = root.ctlCommand
      if (root.ctlAddr !== "" && root.ctlAddr !== "127.0.0.1:18432") {
        // friday-ctl picks the default addr from the daemon itself; only pass
        // an override when the caller configured a non-default one.
        base += " --addr " + root.ctlAddr
      }
      root.bar.run(base + " " + args)
      return true
    }
    // Fall back to the on-PATH binary with a shell escape for the goal.
    root.bar.run("friday-ctl goal " + JSON.stringify(String(goal || "")))
    return true
  }

  property string ctlCommand: "friday-ctl"
  property string ctlAddr: "127.0.0.1:18432"

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): string { state.load(); return "ok" }
    function status(): string { return state.status }
    function goal(text: string): string {
      root.ctlGoal(text)
      return root.ctlGoalPending ? "sent" : "unavailable"
    }
  }
  property bool ctlGoalPending: false

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: state.running ? "🧠" : "⚠"
    // Running FRIDAY lights the accent; a down daemon turns the glyph urgent.
    active: state.running ? root.panelActive() : true
    onPressed: function(buttonCode) { root.toggle() }
  }

  // The icon only has three stable glyphs available from the panel palette;
  // the real status lives in the panel text, not the button tint.
  function panelActive() { return false }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    contentWidth: panel.fittedContentWidth(Style.space(420))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(520))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onActivateRequested: root.ctlGoal(goalField.text)
      onTextKey: function(t) {
        if (t === "r" || t === "R") state.load()
        if (t === "c" || t === "C") root.ctlGoal(goalField.text)
      }
    }

    Column {
      id: column
      anchors.fill: parent
      anchors.margins: Style.space(8)
      spacing: Style.space(6)

      Text {
        text: "FRIDAY"
        font.pixelSize: Style.font.sizes.panel
        font.bold: true
        color: root.foreground
      }

      Text {
        text: state.present
          ? ("daemon " + state.status + (state.stopping ? " (stopping)" : ""))
          : "daemon not detected"
        font.pixelSize: Style.font.sizes.subtext
        color: state.running ? root.accent : root.urgent
      }

      Rectangle {
        width: parent.width
        height: 1
        color: root.dim
      }

      Text {
        visible: state.present && state.currentGoal !== ""
        text: "CURRENT: " + state.currentPhase
        font.pixelSize: Style.font.sizes.subtext
        color: root.foreground
        wrapMode: Text.WordWrap
        width: parent.width
      }
      Text {
        visible: state.present && state.currentGoal !== ""
        text: state.currentGoal
        font.pixelSize: Style.font.sizes.text
        color: root.foreground
        wrapMode: Text.WordWrap
        width: parent.width
      }

      // Cognition slots, shown truthfully only when the daemon populated them.
      Row {
        visible: state.present && state.voice !== "off"
        spacing: Style.space(4)
        Text { text: "voice"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text { text: state.voice; font.pixelSize: Style.font.sizes.subtext; color: root.foreground }
      }
      Row {
        visible: state.present && state.cpu >= 0
        spacing: Style.space(4)
        Text { text: "cpu"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text {
          text: state.cpu.toFixed(1) + "%"
          font.pixelSize: Style.font.sizes.subtext
          color: root.foreground
        }
        Text { text: "mem"; font.pixelSize: Style.font.sizes.subtext; color: root.dim }
        Text {
          text: state.memUsedMb.toFixed(0) + " / " + state.memTotalMb.toFixed(0) + " MB"
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

      Rectangle {
        width: parent.width
        height: 1
        color: root.dim
      }

      Row {
        width: parent.width
        spacing: Style.space(4)
        TextField {
          id: goalField
          width: parent.width - sendButton.width - parent.spacing
          height: Style.space(24)
          placeholderText: "one controlled command…"
          color: root.foreground
          onAccepted: root.ctlGoal(text)
        }
        Button {
          id: sendButton
          width: Style.space(48)
          height: goalField.height
          text: "SEND"
          onClicked: root.ctlGoal(goalField.text)
        }
      }
      Text {
        text: "enter a goal, then c to run / r to refresh"
        font.pixelSize: Style.font.sizes.subtext
        color: root.dim
      }
    }
  }
}
