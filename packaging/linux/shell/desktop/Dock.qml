import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The dock (shell.json "dock": true; Look > Dock, or `friday shell dock on`;
// off unless turned on): your pinned apps ("dockApps", desktop entry ids)
// and the ones running, along the bottom's middle. It stays out of the way:
// the pointer at the bottom edge brings it up, and it slides away once the
// pointer leaves. A click raises a running app or starts a pinned one; the
// dot under an icon means it's running, the longer one that it's in front.
// An idea from end-4/dots-hyprland's dock (see CREDITS.md).
PanelWindow {
  id: dock

  required property ShellScreen modelData
  screen: modelData

  // Up while the pointer is on the edge, or anywhere from the dock down to
  // the edge (no gap to cross on the way up).
  readonly property bool shown: edge.hovered || zoneHover.hovered
  // The app under the pointer, named above the dock.
  property string hoverName: ""
  property real reveal: shown ? 1 : 0
  Behavior on reveal { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

  anchors { bottom: true; left: true; right: true }
  // The dock, the gap under it and room above it for the app's name.
  implicitHeight: 104
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-dock"
  WlrLayershell.layer: WlrLayer.Top
  // Only the edge strip, and the dock while it's up, take the pointer.
  mask: Region {
    item: edgeStrip
    Region { item: dock.reveal > 0.01 ? zone : null }
  }

  Item {
    id: edgeStrip
    anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
    width: Math.max(body.width, 240)
    height: 3
    HoverHandler { id: edge }
  }

  Label {
    visible: dock.reveal > 0.5 && zoneHover.hovered && dock.hoverName !== ""
    anchors { horizontalCenter: zone.horizontalCenter; bottom: zone.top; bottomMargin: 6 }
    text: dock.hoverName
    font.pixelSize: Theme.fontS + 1
    color: Theme.text
  }

  // The dock and the gap under it down to the edge: hovering any of it keeps
  // the dock up. Off the bottom when hidden; 10 px above the edge when up.
  Item {
    id: zone
    anchors.horizontalCenter: parent.horizontalCenter
    y: parent.height - (body.height + 10) * dock.reveal
    width: body.width
    height: parent.height - y
    HoverHandler { id: zoneHover }

    Rectangle {
      id: body
      width: row.implicitWidth + 20
      height: 58
      radius: Math.min(18, Theme.radius)
      color: Theme.glassSolid
      border.width: 1
      border.color: Theme.glassEdge
      opacity: dock.reveal
      visible: dock.reveal > 0.01

      RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: 6
        Repeater {
          model: Apps.items
          Item {
            id: slot
            required property var modelData
            readonly property var wins: Apps.windowsOf(modelData)
            readonly property bool front: wins.some(w => w.activated)
            implicitWidth: 44
            implicitHeight: 48

            Rectangle {
              anchors.centerIn: icon
              width: 42; height: 42
              radius: 12
              color: tap.hovered ? Theme.raised : "transparent"
            }
            Image {
              id: icon
              anchors.horizontalCenter: parent.horizontalCenter
              y: 2 - (tap.hovered && Theme.motion ? 3 : 0)
              Behavior on y { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
              width: 34; height: 34
              sourceSize: Qt.size(64, 64)
              source: Apps.icon(slot.modelData)
              asynchronous: true
            }
            Glyph {
              visible: icon.status !== Image.Ready
              anchors.centerIn: icon
              text: Icons.apps
              size: 22
              color: Theme.faint
            }
            // Running: a dot; in front: a longer one in the accent.
            Rectangle {
              visible: slot.wins.length > 0
              anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom }
              width: slot.front ? 14 : 5
              height: 4
              radius: 2
              color: slot.front ? Theme.accent : Theme.subtext
              Behavior on width { NumberAnimation { duration: Theme.fast } }
            }
            HoverHandler { id: tap }
            TapHandler { onTapped: Apps.activate(slot.modelData) }
            Binding {
              when: tap.hovered
              target: dock
              property: "hoverName"
              value: Apps.name(slot.modelData)
            }
          }
        }
      }
    }
  }
}
