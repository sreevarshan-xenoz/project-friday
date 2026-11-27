import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The rail style's bar (Theme.rail): one slim column down the left edge,
// for wide screens and niri's sideways strip. Top: apps, FRIDAY (its orb
// shows what it's doing; click to open it) and the workspaces, stacked.
// Bottom: the time in two lines, the status pill on its side, and power.
// Drawers open beside it. Inspired by the side bars of caelestia-dots/shell,
// snowarch/iNiR and end-4/dots-hyprland (ideas, see CREDITS.md).
PanelWindow {
  id: rail

  required property ShellScreen modelData
  screen: modelData

  anchors { top: true; bottom: true; left: true }
  implicitWidth: Theme.railWidth
  exclusiveZone: Theme.railWidth
  color: Theme.glass
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // A hairline on the screen side, so the rail reads as one edge.
  Rectangle {
    anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
    width: 1
    color: Theme.glassEdge
  }

  ColumnLayout {
    anchors { top: parent.top; horizontalCenter: parent.horizontalCenter; topMargin: 10 }
    spacing: 12

    IconButton {
      Layout.alignment: Qt.AlignHCenter
      tooltip: "Apps"
      icon: Icons.apps
      size: 28
      iconSize: 16
      active: Shell.isOpen("launcher", rail.modelData.name)
      onClicked: Shell.toggle("launcher", rail.modelData.name)
    }
    Item {
      Layout.alignment: Qt.AlignHCenter
      implicitWidth: 28
      implicitHeight: 28
      Orb { anchors.centerIn: parent; size: 22 }
      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: Shell.toggle("friday", rail.modelData.name)
      }
    }
    Workspaces {
      Layout.alignment: Qt.AlignHCenter
      screen: rail.modelData
      vertical: true
    }
  }

  ColumnLayout {
    anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter; bottomMargin: 10 }
    spacing: 12

    // The time in two lines; a click opens the calendar.
    Item {
      Layout.alignment: Qt.AlignHCenter
      implicitWidth: clockCol.implicitWidth
      implicitHeight: clockCol.implicitHeight
      SystemClock { id: time; precision: SystemClock.Minutes }
      ColumnLayout {
        id: clockCol
        spacing: 0
        Label {
          Layout.alignment: Qt.AlignHCenter
          text: Qt.formatDateTime(time.date, Config.clock24h ? "HH" : "h")
          font.family: Theme.mono
          font.pixelSize: Theme.fontM
          font.weight: Font.DemiBold
        }
        Label {
          Layout.alignment: Qt.AlignHCenter
          text: Qt.formatDateTime(time.date, "mm")
          font.family: Theme.mono
          font.pixelSize: Theme.fontM
          color: Theme.subtext
        }
      }
      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: Shell.toggle("calendar", rail.modelData.name)
      }
    }
    StatusCluster {
      Layout.alignment: Qt.AlignHCenter
      vertical: true
      onClicked: Shell.toggle("control", rail.modelData.name)
    }
    IconButton {
      Layout.alignment: Qt.AlignHCenter
      tooltip: "Power and session"
      icon: Icons.power
      size: 28
      iconSize: 15
      tint: Theme.error
      active: Shell.isOpen("session", rail.modelData.name)
      onClicked: Shell.toggle("session", rail.modelData.name)
    }
  }
}
