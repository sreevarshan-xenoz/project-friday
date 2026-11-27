import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The workspace overview (Super+Tab, `friday shell overview`): every
// workspace on this screen as a card with live pictures of its windows where
// they sit. Click a window to go to it, a card to go there; drag a window
// onto another card to move it; arrows and Enter pick a workspace; Escape or
// a click outside closes. The pictures exist only while it's open.
PanelWindow {
  id: ov

  required property ShellScreen modelData
  screen: modelData

  property bool open: false
  visible: open || fade.running
  readonly property bool hyprland: !!Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE")
  readonly property HyprlandMonitor monitor: hyprland ? Hyprland.monitorFor(modelData) : null
  readonly property int activeId: monitor && monitor.activeWorkspace ? monitor.activeWorkspace.id : 1
  property int picked: activeId

  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-overview"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

  function toggle() {
    if (!hyprland || (!open && !Config.featureOn("workspace-overview"))) return;
    if (!open) { Hyprland.refreshToplevels(); picked = activeId; }
    open = !open;
  }

  // Close first, then go: letting go of the keyboard makes Hyprland refocus
  // the last window, which would pull the view back if it came after.
  property string pending: ""
  function closeThen(dispatch) { pending = dispatch; open = false; after.restart(); }
  function go(id) { closeThen("hl.dsp.focus({ workspace = \"" + id + "\" })"); }
  function focusWindow(address) { closeThen("hl.dsp.focus({ window = \"address:0x" + address + "\" })"); }
  Timer {
    id: after
    interval: 120
    onTriggered: if (ov.pending) { Quickshell.execDetached(["hyprctl", "dispatch", ov.pending]); ov.pending = ""; }
  }
  function moveWindow(address, id) {
    Quickshell.execDetached(["hyprctl", "dispatch", "hl.dsp.window.move({ workspace = \"" + id + "\", window = \"address:0x" + address + "\" })"]);
    Qt.callLater(() => { ov.picked = id; Hyprland.refreshToplevels(); });
  }

  // How many cards: the configured slots, or more if windows live further on.
  readonly property int slots: {
    let n = Config.workspaces;
    for (const w of Hyprland.workspaces.values)
      if (w.id > 0 && (!w.monitor || !monitor || w.monitor === monitor)) n = Math.max(n, w.id);
    return Math.max(n, activeId);
  }

  // The screen as a card: its size and where it starts in the layout.
  readonly property real monX: monitor && monitor.lastIpcObject ? monitor.lastIpcObject.x || 0 : 0
  readonly property real monY: monitor && monitor.lastIpcObject ? monitor.lastIpcObject.y || 0 : 0
  readonly property real monW: Math.max(1, monitor ? monitor.width / (monitor.scale || 1) : width)
  readonly property real monH: Math.max(1, monitor ? monitor.height / (monitor.scale || 1) : height)

  Shortcut { sequence: "Escape"; enabled: ov.open; onActivated: ov.open = false }
  Shortcut { sequence: "Left"; enabled: ov.open; onActivated: ov.picked = Math.max(1, ov.picked - 1) }
  Shortcut { sequence: "Right"; enabled: ov.open; onActivated: ov.picked = Math.min(ov.slots, ov.picked + 1) }
  Shortcut { sequence: "Return"; enabled: ov.open; onActivated: ov.go(ov.picked) }

  IpcHandler {
    target: "overview"
    function toggle(): void { if (Shell.focusedScreen() === ov.modelData.name) ov.toggle(); }
    function close(): void { ov.open = false; }
    function isOpen(): string { return ov.open ? "true" : "false"; }
  }

  // The dimmed desktop; a click on it closes.
  Rectangle {
    id: dim
    anchors.fill: parent
    color: Theme.alpha(Theme.bg, 0.88)
    opacity: ov.open ? 1 : 0
    Behavior on opacity { NumberAnimation { id: fade; duration: Theme.medium; easing.type: Theme.easing } }
    MouseArea { anchors.fill: parent; onClicked: ov.open = false }
  }

  Flow {
    id: grid
    readonly property int cols: ov.slots <= 5 ? ov.slots : Math.ceil(ov.slots / 2)
    readonly property real cardW: Math.min(460, (ov.width - 160 - (cols - 1) * spacing) / cols)
    readonly property real cardH: cardW * ov.monH / ov.monW
    width: cols * cardW + (cols - 1) * spacing
    anchors.centerIn: parent
    spacing: 28
    opacity: dim.opacity

    Repeater {
      model: ov.open ? ov.slots : 0

      Rectangle {
        id: card
        required property int index
        readonly property int wsId: index + 1
        readonly property bool current: wsId === ov.activeId
        readonly property bool chosen: wsId === ov.picked
        readonly property real k: grid.cardW / ov.monW
        property bool dragging: false
        z: dragging ? 5 : 0

        width: grid.cardW
        height: grid.cardH + 30
        color: "transparent"

        // They arrive one after another, springing up into place.
        opacity: 0
        scale: 0.86
        Component.onCompleted: arrive.start()
        SequentialAnimation {
          id: arrive
          PauseAnimation { duration: Theme.motion ? card.index * 35 : 0 }
          ParallelAnimation {
            NumberAnimation { target: card; property: "opacity"; to: 1; duration: Theme.medium; easing.type: Theme.easing }
            NumberAnimation { target: card; property: "scale"; to: 1; duration: Theme.slow; easing.type: Theme.spring; easing.overshoot: 1.2 }
          }
        }

        Rectangle {
          id: frame
          width: grid.cardW
          height: grid.cardH
          radius: 14
          clip: !card.dragging
          color: Theme.surface
          border.width: card.chosen || drop.containsDrag ? 2 : 1
          border.color: drop.containsDrag ? Theme.signal : card.chosen ? Theme.accent : card.current ? Theme.alpha(Theme.accent, 0.5) : Theme.glassEdge
          scale: frameHover.hovered ? 1.02 : 1
          Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

          HoverHandler { id: frameHover; onHoveredChanged: if (hovered) ov.picked = card.wsId }
          // One tap handler: the window under the pointer, or else the workspace.
          TapHandler {
            onTapped: p => {
              const w = shots.childAt(p.position.x, p.position.y);
              if (w && w.address) ov.focusWindow(w.address); else ov.go(card.wsId);
            }
          }
          DropArea {
            id: drop
            anchors.fill: parent
            keys: ["friday-window"]
            onDropped: d => { if (d.source && d.source.address) ov.moveWindow(d.source.address, card.wsId); }
          }

          Label {
            anchors.centerIn: parent
            visible: windows.count === 0
            text: "Empty"
            color: Theme.faint
          }

          // Each window where it sits, a live picture of it.
          Item {
            id: shots
            anchors.fill: parent
            Repeater {
              id: windows
              model: Hyprland.toplevels.values.filter(t => t.workspace && t.workspace.id === card.wsId && t.lastIpcObject && t.lastIpcObject.at)
  
              Item {
                id: win
                required property var modelData
                readonly property string address: modelData.address
                readonly property var ipc: modelData.lastIpcObject
                x: (ipc.at[0] - ov.monX) * card.k
                y: (ipc.at[1] - ov.monY) * card.k
                width: Math.max(12, ipc.size[0] * card.k)
                height: Math.max(12, ipc.size[1] * card.k)
                z: dragger.active ? 10 : winHover.hovered ? 2 : 1
  
                Drag.active: dragger.active
                Drag.keys: ["friday-window"]
                Drag.source: win
                Drag.hotSpot.x: width / 2
                Drag.hotSpot.y: height / 2
  
                Rectangle {
                  anchors.fill: parent
                  radius: 6
                  color: Theme.raised
                  border.width: winHover.hovered ? 2 : 1
                  border.color: winHover.hovered ? Theme.accent : Theme.glassEdge
                  clip: true
                  // Live until the first frame is in, then still; the hovered
                  // one stays live. Live pictures of every window cost
                  // Hyprland about 13% CPU, still ones nothing.
                  ScreencopyView {
                    id: shot
                    anchors.fill: parent
                    anchors.margins: 1
                    captureSource: ov.open ? win.modelData.wayland : null
                    live: !hasContent || winHover.hovered
                  }
                  // The app's name over a window with no picture yet (just
                  // after a shell restart), its title on hover.
                  Label {
                    anchors.centerIn: parent
                    visible: win.width > 60
                    width: parent.width - 8
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: winHover.hovered ? win.modelData.title : shot.hasContent ? "" : (win.ipc.class || win.modelData.title)
                    font.pixelSize: Theme.fontS
                    style: Text.Outline
                    styleColor: Theme.bg
                  }
                }
                scale: dragger.active ? 0.8 : winHover.hovered ? 1.04 : 1
                Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
  
                HoverHandler { id: winHover }
                DragHandler {
                  id: dragger
                  onActiveChanged: {
                    card.dragging = active;
                    if (!active) { win.Drag.drop(); win.x = Qt.binding(() => (win.ipc.at[0] - ov.monX) * card.k); win.y = Qt.binding(() => (win.ipc.at[1] - ov.monY) * card.k); }
                  }
                }
              }
            }
          }
        }

        RowLayout {
          anchors.top: frame.bottom
          anchors.topMargin: 8
          anchors.horizontalCenter: frame.horizontalCenter
          spacing: 6
          Rectangle { width: 6; height: 6; radius: 3; color: Theme.accent; visible: card.current }
          Label {
            text: "Workspace " + card.wsId
            color: card.chosen ? Theme.text : Theme.subtext
            font.weight: card.chosen ? Font.DemiBold : Font.Normal
          }
        }
      }
    }
  }
}
