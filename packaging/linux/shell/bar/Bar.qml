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
//   keystone  one notch hanging from the middle of the top edge, the groups
//           pulled in around FRIDAY's island (the rest of the edge is free)
PanelWindow {
  id: bar

  required property ShellScreen modelData
  screen: modelData

  readonly property bool island: Theme.style === "island"

  anchors { top: !Theme.barAtBottom; bottom: Theme.barAtBottom; left: true; right: true }
  // Room for the island to open below the bar (it's masked, so clicks pass).
  implicitHeight: (Theme.floating ? Theme.barReach : Theme.barHeight + Theme.fillet) + 40
  exclusiveZone: Theme.floating ? Theme.barReach : Theme.barHeight
  color: "transparent"
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // Only the bar takes input: the strip, or in the island style just its
  // pills, so clicks between them reach the windows below.
  mask: Region {
    item: bar.island ? null : Theme.keystone ? notch : strip
    Region { item: bar.island ? leftPill : null }
    Region { item: centre.capsule }
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
    color: bar.island || Theme.keystone ? "transparent" : Theme.glass
    border.width: Theme.style === "dock" ? 1 : 0
    border.color: Theme.glassEdge
  }
  // Keystone: the notch, flush with the top edge, round at the bottom, with
  // a small curve on each side where it meets the edge.
  Rectangle {
    id: notch
    visible: Theme.keystone
    x: leftGroup.x - 16
    width: rightGroup.x + rightGroup.width + 16 - x
    y: -radius
    height: Theme.barHeight + radius
    radius: 16
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
  }
  Corner { visible: Theme.keystone; size: 10; leftSide: false; x: notch.x - size; y: 0 }
  Corner { visible: Theme.keystone; size: 10; leftSide: true; x: notch.x + notch.width; y: 0 }

  Corner { visible: !Theme.floating && !Theme.keystone; leftSide: true; anchors.top: strip.bottom; anchors.left: parent.left }
  Corner { visible: !Theme.floating && !Theme.keystone; leftSide: false; anchors.top: strip.bottom; anchors.right: parent.right }

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
  Pill { id: rightPill; group: rightGroup }

  // Left.
  RowLayout {
    id: leftGroup
    // Keystone: just left of the island instead of at the screen's edge.
    anchors {
      left: Theme.keystone ? undefined : strip.left
      right: Theme.keystone ? centre.left : undefined
      leftMargin: 10
      rightMargin: 14
      verticalCenter: strip.verticalCenter
    }
    spacing: 10

    IconButton {
      tooltip: "Apps"
      icon: Icons.apps
      size: 26
      iconSize: 16
      active: Shell.isOpen("launcher", bar.modelData.name)
      onClicked: Shell.toggle("launcher", bar.modelData.name)
    }
    Workspaces { id: workspaces; screen: bar.modelData }
    WindowTitle { visible: !Theme.keystone; Layout.maximumWidth: Math.max(0, centre.x - strip.x - 260) }
  }

  // Centre: the aperture island (bar/Island.qml), centred on the bar but
  // never over the apps button and workspaces on a narrow screen. It drops
  // below the bar (or rises above the dock) when it opens.
  Island {
    id: centre
    screenName: bar.modelData.name
    growUp: Theme.barAtBottom
    y: strip.y + 4
    x: Theme.keystone ? Math.round(strip.x + strip.width / 2 - width / 2)
      : Math.round(Math.max(leftGroup.x + workspaces.x + workspaces.width + 24,
      strip.x + strip.width / 2 - width / 2))
  }

  // Right.
  RowLayout {
    id: rightGroup
    // Keystone: just right of the island.
    anchors {
      right: Theme.keystone ? undefined : strip.right
      left: Theme.keystone ? centre.right : undefined
      rightMargin: 10
      leftMargin: 14
      verticalCenter: strip.verticalCenter
    }
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
