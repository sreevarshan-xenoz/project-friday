import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// One per screen: the surface drawers grow out of. In the ember style it
// sits right under the bar, and the open drawer's body unrolls downward from
// the bar's edge with its concave shoulders, so the bar seems to extend. In
// the floating styles a drawer is a rounded card that rolls out a gap away
// from the bar (down from an island bar, up from the dock). A click anywhere
// else, or Escape, closes it.
PanelWindow {
  id: host

  required property ShellScreen modelData
  screen: modelData

  // Which drawer this screen shows; `shown` lingers while it rolls back up.
  readonly property string current: Shell.screen === modelData.name ? Shell.drawer : ""
  property string shown: ""
  // From one open drawer straight to another: the body slides and resizes
  // to the new one while its content cross-fades, instead of jumping.
  property bool switching: false
  onCurrentChanged: {
    if (current !== "" && shown !== "" && shown !== current && reveal > 0.99) {
      switching = true;
      settle.restart();
      swapFade.restart();
    }
    if (current !== "") shown = current;
  }
  Timer { id: settle; interval: Theme.medium + 40; onTriggered: host.switching = false }
  property real swap: 1
  NumberAnimation { id: swapFade; target: host; property: "swap"; from: 0; to: 1; duration: Theme.medium; easing.type: Theme.easing }

  property real reveal: current !== "" ? 1 : 0
  // Quick and plain both ways: no overshoot on something opened this often.
  Behavior on reveal {
    NumberAnimation {
      duration: host.current !== "" ? Theme.medium : Theme.fast + 40
      easing.type: Theme.easing
    }
  }

  readonly property var specs: ({
    friday: { align: "center", source: "FridayDrawer.qml", dim: false },
    calendar: { align: "center", source: "CalendarDrawer.qml", dim: false },
    launcher: { align: "left", source: "LauncherDrawer.qml", dim: false },
    control: { align: "right", source: "ControlDrawer.qml", dim: false },
    session: { align: "right", source: "SessionDrawer.qml", dim: true }
  })
  readonly property var spec: specs[shown] || specs.friday
  // Orbit keeps FRIDAY and the clock in its right pill: what opens from the
  // middle elsewhere opens under that pill.
  readonly property string align: Theme.style === "orbit" && spec.align === "center" ? "right" : spec.align

  visible: current !== "" || reveal > 0.01
  onVisibleChanged: if (!visible) shown = ""

  anchors { top: true; left: true; right: true; bottom: true }
  margins.top: Theme.rail ? Theme.gap : Theme.barAtBottom ? 0 : Theme.floating ? Theme.barReach + Theme.gap : Theme.barHeight + (Theme.sharp ? 6 : 0)
  margins.left: Theme.leftReach
  margins.bottom: Theme.barAtBottom ? Theme.barReach + Theme.gap : 0
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

    readonly property real fullW: loader.item ? loader.item.implicitWidth + 2 * Theme.pad : 0
    readonly property real fullH: loader.item ? loader.item.implicitHeight + 2 * Theme.pad : 0
    // Follow the drawer's size; eased only while switching drawers.
    property real bodyW: fullW
    property real bodyH: fullH
    Behavior on bodyW { enabled: host.switching; NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    Behavior on bodyH { enabled: host.switching; NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    Behavior on x { enabled: host.switching; NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

    // Plain cards (floating styles, the rail, the statusline) or, in the
    // ember styles, a body with shoulders that grows out of the bar.
    readonly property bool card: Theme.floating || Theme.rail || Theme.statusline || Theme.taskbar || Theme.sharp
    // Ember: room for the shoulders either side. A card: just the card.
    readonly property real shoulder: card ? 0 : Theme.fillet
    // A card's distance from the screen's side.
    readonly property real side: Theme.floating ? Theme.barInset : Theme.statusline || Theme.taskbar ? Theme.gap : Theme.sharp ? 8 : -Theme.fillet

    width: bodyW + 2 * shoulder
    height: bodyH * host.reveal
    // Rising from the dock, it grows upward from the host's bottom edge.
    y: Theme.barAtBottom ? host.height - height : 0
    // Ember: at a screen edge the outer shoulder is pushed off-screen, so the
    // body runs flush with the edge under the bar's own corner. Floating: the
    // card keeps the bar's inset from the edge.
    // The rail: every drawer opens beside it, at the top.
    x: Theme.rail ? Theme.gap
      : host.align === "left" ? side
      : host.align === "right" ? host.width - width - side
      : Math.round((host.width - width) / 2)

    // Swallow clicks on the body so they don't reach the close handler.
    MouseArea { anchors.fill: parent }

    DrawerShape { anchors.fill: parent; visible: !drawer.card }
    Rectangle {
      anchors.fill: parent
      visible: drawer.card
      radius: Math.min(Theme.radius, height / 2)
      color: Theme.glass
      border.width: 1
      border.color: Theme.glassEdge
    }
    // Neon: two small accent brackets, top left and bottom right.
    Repeater {
      model: Theme.style === "neon" ? [0, 1] : []
      Item {
        id: bracket
        required property int modelData
        readonly property bool far: modelData === 1
        x: far ? drawer.width - 14 : 0
        y: far ? drawer.height - 14 : 0
        width: 14
        height: 14
        opacity: host.reveal
        Rectangle { width: 14; height: 1; color: Theme.accent; y: bracket.far ? 13 : 0 }
        Rectangle { width: 1; height: 14; color: Theme.accent; x: bracket.far ? 13 : 0 }
      }
    }

    Item {
      id: content
      x: drawer.shoulder + Theme.pad
      y: Theme.pad - (1 - host.reveal) * (Theme.barAtBottom ? -12 : 12)
      width: drawer.bodyW - 2 * Theme.pad
      height: drawer.bodyH - 2 * Theme.pad
      opacity: Math.max(0, (host.reveal - 0.35) / 0.65) * host.swap
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
