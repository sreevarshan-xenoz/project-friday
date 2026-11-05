import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// One per screen: the surface drawers grow out of. It sits right under the
// bar, and the open drawer's body unrolls downward from the bar's edge with
// its concave shoulders, so the bar seems to extend. A click anywhere else,
// or Escape, closes it.
PanelWindow {
  id: host

  required property ShellScreen modelData
  screen: modelData

  // Which drawer this screen shows; `shown` lingers while it rolls back up.
  readonly property string current: Shell.screen === modelData.name ? Shell.drawer : ""
  property string shown: ""
  onCurrentChanged: if (current !== "") shown = current

  property real reveal: current !== "" ? 1 : 0
  Behavior on reveal { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

  readonly property var specs: ({
    friday: { align: "center", source: "FridayDrawer.qml", dim: false },
    calendar: { align: "center", source: "CalendarDrawer.qml", dim: false },
    launcher: { align: "left", source: "LauncherDrawer.qml", dim: false },
    control: { align: "right", source: "ControlDrawer.qml", dim: false },
    session: { align: "right", source: "SessionDrawer.qml", dim: true }
  })
  readonly property var spec: specs[shown] || specs.friday

  visible: current !== "" || reveal > 0.01
  onVisibleChanged: if (!visible) shown = ""

  anchors { top: true; left: true; right: true; bottom: true }
  margins.top: Theme.barHeight
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-drawer"
  WlrLayershell.layer: WlrLayer.Top
  WlrLayershell.keyboardFocus: current !== "" ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

  Rectangle {
    anchors.fill: parent
    color: Theme.alpha("#000000", host.spec.dim ? 0.35 * host.reveal : 0)
  }

  MouseArea {
    anchors.fill: parent
    onClicked: Shell.close()
  }

  Item {
    id: drawer

    readonly property real bodyW: loader.item ? loader.item.implicitWidth + 2 * Theme.pad : 0
    readonly property real bodyH: loader.item ? loader.item.implicitHeight + 2 * Theme.pad : 0

    width: bodyW + 2 * Theme.fillet
    height: bodyH * host.reveal
    // At a screen edge the outer shoulder is pushed off-screen, so the body
    // runs flush with the edge under the bar's own corner.
    x: host.spec.align === "left" ? -Theme.fillet
      : host.spec.align === "right" ? host.width - width + Theme.fillet
      : Math.round((host.width - width) / 2)

    // Swallow clicks on the body so they don't reach the close handler.
    MouseArea { anchors.fill: parent }

    DrawerShape { anchors.fill: parent }

    Item {
      id: content
      x: Theme.fillet + Theme.pad
      y: Theme.pad - (1 - host.reveal) * 12
      width: drawer.bodyW - 2 * Theme.pad
      height: drawer.bodyH - 2 * Theme.pad
      opacity: Math.max(0, (host.reveal - 0.35) / 0.65)
      clip: true

      Loader {
        id: loader
        source: host.shown !== "" ? host.spec.source : ""
        focus: true
        onLoaded: item.forceActiveFocus()
      }

      Keys.onEscapePressed: Shell.close()
    }
  }

  Connections {
    target: loader.item
    ignoreUnknownSignals: true
    function onDone() { Shell.close(); }
  }
}
