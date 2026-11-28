import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The taskbar style's bar (Theme.taskbar): one bar along the bottom, the
// way Windows lays it out. In the middle: the apps button, FRIDAY (its orb;
// click to open it) and the apps, pinned ("dockApps") then running, with a
// mark under the running ones; a click raises or starts one. Left: the
// workspaces. Right: the tray, the status and the time over the date. After
// end-4/dots-hyprland's Windows-style "waffle" bar (an idea, see CREDITS.md).
PanelWindow {
  id: task

  required property ShellScreen modelData
  screen: modelData

  anchors { bottom: true; left: true; right: true }
  implicitHeight: Theme.barHeight
  exclusiveZone: Theme.barHeight
  color: Theme.glass
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // A hairline along the top edge.
  Rectangle {
    anchors { top: parent.top; left: parent.left; right: parent.right }
    height: 1
    color: Theme.glassEdge
  }

  Workspaces {
    anchors { left: parent.left; leftMargin: 12; verticalCenter: parent.verticalCenter }
    screen: task.modelData
  }

  RowLayout {
    anchors.centerIn: parent
    spacing: 4

    IconButton {
      tooltip: "Apps"
      icon: Icons.apps
      size: 38
      iconSize: 18
      active: Shell.isOpen("launcher", task.modelData.name)
      onClicked: Shell.toggle("launcher", task.modelData.name)
    }
    Item {
      implicitWidth: 38
      implicitHeight: 38
      Rectangle {
        anchors.fill: parent
        radius: 8
        color: fridayHover.hovered ? Theme.overlay : "transparent"
      }
      Orb { anchors.centerIn: parent; size: 24 }
      HoverHandler { id: fridayHover }
      TapHandler { onTapped: Shell.toggle("friday", task.modelData.name) }
    }
    Repeater {
      model: Apps.items
      Item {
        id: slot
        required property var modelData
        readonly property var wins: Apps.windowsOf(modelData)
        readonly property bool front: wins.some(w => w.activated)
        implicitWidth: 38
        implicitHeight: 38
        Rectangle {
          anchors.fill: parent
          radius: 8
          color: slot.front ? Theme.raised : hover.hovered ? Theme.overlay : "transparent"
        }
        Image {
          id: icon
          anchors.centerIn: parent
          width: 24; height: 24
          sourceSize: Qt.size(48, 48)
          source: Apps.icon(slot.modelData)
          asynchronous: true
        }
        Glyph {
          visible: icon.status !== Image.Ready
          anchors.centerIn: parent
          text: Icons.apps
          size: 18
          color: Theme.faint
        }
        // Running: a short mark; in front: a longer one in the accent.
        Rectangle {
          visible: slot.wins.length > 0
          anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 1 }
          width: slot.front ? 16 : 6
          height: 3
          radius: 1.5
          color: slot.front ? Theme.accent : Theme.subtext
          Behavior on width { NumberAnimation { duration: Theme.fast } }
        }
        HoverHandler { id: hover }
        TapHandler { onTapped: Apps.activate(slot.modelData) }
        Accessible.role: Accessible.Button
        Accessible.name: Apps.name(slot.modelData)
      }
    }
  }

  RowLayout {
    anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
    spacing: 12
    Tray {}
    StatusCluster { onClicked: Shell.toggle("control", task.modelData.name) }
    // The time over the date; a click opens the calendar.
    Item {
      implicitWidth: clockCol.implicitWidth
      implicitHeight: clockCol.implicitHeight
      SystemClock { id: time; precision: SystemClock.Minutes }
      ColumnLayout {
        id: clockCol
        spacing: 0
        Label {
          Layout.alignment: Qt.AlignRight
          text: Qt.formatDateTime(time.date, Config.clock24h ? "HH:mm" : "h:mm ap")
          font.pixelSize: Theme.fontS + 1
        }
        Label {
          Layout.alignment: Qt.AlignRight
          text: Qt.formatDateTime(time.date, "d/M/yyyy")
          font.pixelSize: Theme.fontS
          color: Theme.subtext
        }
      }
      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: Shell.toggle("calendar", task.modelData.name)
      }
    }
    IconButton {
      tooltip: "Power and session"
      icon: Icons.power
      size: 28
      iconSize: 15
      tint: Theme.error
      active: Shell.isOpen("session", task.modelData.name)
      onClicked: Shell.toggle("session", task.modelData.name)
    }
  }
}
