import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The bar. Left: launcher and workspaces. Centre: FRIDAY and the clock.
// Right: tray, media, the status pill and power. Where it sits and its shape
// follow the style (Theme.style):
//   ember   flush with the top edge, flowing into the screen through two
//           concave corners
//   island  the three groups as separate floating pills along the top
//   dock    one floating bar along the bottom
PanelWindow {
  id: bar

  required property ShellScreen modelData
  screen: modelData

  readonly property bool island: Theme.style === "island"

  anchors { top: !Theme.barAtBottom; bottom: Theme.barAtBottom; left: true; right: true }
  implicitHeight: Theme.floating ? Theme.barReach : Theme.barHeight + Theme.fillet
  exclusiveZone: Theme.floating ? Theme.barReach : Theme.barHeight
  color: "transparent"
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // Only the bar takes input: the strip, or in the island style just its
  // pills, so clicks between them reach the windows below.
  mask: Region {
    item: bar.island ? null : strip
    Region { item: bar.island ? leftPill : null }
    Region { item: bar.island ? centrePill : null }
    Region { item: bar.island ? rightPill : null }
  }

  Rectangle {
    id: strip
    anchors {
      top: Theme.barAtBottom ? undefined : parent.top
      bottom: Theme.barAtBottom ? parent.bottom : undefined
      left: parent.left
      right: parent.right
      margins: Theme.barInset
    }
    height: Theme.barHeight
    radius: Theme.floating ? height / 2 : 0
    color: bar.island ? "transparent" : Theme.glass
    border.width: Theme.style === "dock" ? 1 : 0
    border.color: Theme.glassEdge
  }
  Corner { visible: !Theme.floating; leftSide: true; anchors.top: strip.bottom; anchors.left: parent.left }
  Corner { visible: !Theme.floating; leftSide: false; anchors.top: strip.bottom; anchors.right: parent.right }

  // The island style's pills: one behind each group.
  component Pill: Rectangle {
    required property Item group
    visible: bar.island
    x: group.x - 10
    y: strip.y
    width: group.width + 20
    height: strip.height
    radius: height / 2
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
  }
  Pill { id: leftPill; group: leftGroup }
  Pill { id: centrePill; group: centre }
  Pill { id: rightPill; group: rightGroup }

  // Left.
  RowLayout {
    id: leftGroup
    anchors { left: strip.left; leftMargin: 10; verticalCenter: strip.verticalCenter }
    spacing: 10

    IconButton {
      tooltip: "Apps"
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
    id: rightGroup
    anchors { right: strip.right; rightMargin: 10; verticalCenter: strip.verticalCenter }
    spacing: 12

    Tray {}
    MediaChip {}
    StatusCluster {
      onClicked: Shell.toggle("control", bar.modelData.name)
    }
    IconButton {
      tooltip: "Power and session"
      icon: Icons.power
      size: 26
      iconSize: 15
      tint: Theme.error
      active: Shell.isOpen("session", bar.modelData.name)
      onClicked: Shell.toggle("session", bar.modelData.name)
    }
  }
}
