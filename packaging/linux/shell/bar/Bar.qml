import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The bar: flush with the top edge, flowing into the screen through two
// concave corners. Left: launcher and workspaces. Centre: FRIDAY and the
// clock. Right: tray, media, the status pill and power.
PanelWindow {
  id: bar

  required property ShellScreen modelData
  screen: modelData

  anchors { top: true; left: true; right: true }
  implicitHeight: Theme.barHeight + Theme.fillet
  exclusiveZone: Theme.barHeight
  color: "transparent"
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // Only the bar strip takes input; the corners below it are decoration.
  mask: Region { item: strip }

  Rectangle {
    id: strip
    anchors { top: parent.top; left: parent.left; right: parent.right }
    height: Theme.barHeight
    color: Theme.bg
  }
  Corner { leftSide: true; anchors.top: strip.bottom; anchors.left: parent.left }
  Corner { leftSide: false; anchors.top: strip.bottom; anchors.right: parent.right }

  // Left.
  RowLayout {
    anchors { left: strip.left; leftMargin: 10; verticalCenter: strip.verticalCenter }
    spacing: 10

    IconButton {
      icon: Icons.apps
      size: 26
      iconSize: 16
      active: Shell.isOpen("launcher", bar.modelData.name)
      onClicked: Shell.toggle("launcher", bar.modelData.name)
    }
    Workspaces { screen: bar.modelData }
    WindowTitle { Layout.maximumWidth: Math.max(0, bar.width / 2 - centre.width / 2 - 260) }
  }

  // Centre: the orb (FRIDAY drawer) and what it's doing, then the clock
  // (calendar drawer).
  RowLayout {
    id: centre
    anchors.centerIn: strip
    spacing: 12

    Rectangle {
      id: fridayButton
      implicitHeight: 26
      implicitWidth: fridayRow.implicitWidth + 14
      radius: height / 2
      color: Shell.isOpen("friday", bar.modelData.name) ? Theme.raised
        : fridayMouse.containsMouse ? Theme.surface : "transparent"
      Behavior on color { ColorAnimation { duration: Theme.fast } }
      Behavior on implicitWidth { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
      clip: true

      RowLayout {
        id: fridayRow
        anchors.verticalCenter: parent.verticalCenter
        x: 7
        spacing: 8
        Orb { size: 18 }
        Label {
          visible: Friday.mode !== "ready"
          Layout.maximumWidth: 320
          text: Friday.mode === "working" ? (Friday.phase || "working") + (Friday.goal ? " · " + Friday.goal : "")
            : Friday.mode === "stopped" ? "FRIDAY " + Friday.status : "FRIDAY offline"
          color: Friday.mode === "working" ? Theme.accent : Theme.subtext
          font.pixelSize: Theme.fontS + 1
          font.weight: Font.Medium
        }
      }
      MouseArea {
        id: fridayMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: Shell.toggle("friday", bar.modelData.name)
      }
    }

    Label {
      id: clock
      text: Qt.formatDateTime(time.date, Config.clock24h ? "ddd d MMM   HH:mm" : "ddd d MMM   h:mm AP")
      font.family: Theme.mono
      font.pixelSize: Theme.fontM
      font.weight: Font.Medium
      color: clockMouse.containsMouse || Shell.isOpen("calendar", bar.modelData.name) ? Theme.accent : Theme.text
      SystemClock { id: time; precision: SystemClock.Minutes }
      MouseArea {
        id: clockMouse
        anchors.fill: parent
        anchors.margins: -6
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: Shell.toggle("calendar", bar.modelData.name)
      }
    }
  }

  // Right.
  RowLayout {
    anchors { right: strip.right; rightMargin: 10; verticalCenter: strip.verticalCenter }
    spacing: 12

    Tray {}
    MediaChip {}
    StatusCluster {
      onClicked: Shell.toggle("control", bar.modelData.name)
    }
    IconButton {
      icon: Icons.power
      size: 26
      iconSize: 15
      tint: Theme.error
      active: Shell.isOpen("session", bar.modelData.name)
      onClicked: Shell.toggle("session", bar.modelData.name)
    }
  }
}
