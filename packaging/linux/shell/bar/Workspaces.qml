import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.WindowManager
import qs.common
import qs.components
import qs.services

// Workspace slots for this bar's monitor: the active one is a wide ember
// bar, occupied ones a solid dot, empty ones a faint dot. Click to go there,
// scroll to step. On Hyprland from its own IPC (fixed slots); elsewhere (niri,
// and any compositor with the ext-workspace protocol) from Quickshell's
// WindowManager, which lists the workspaces the compositor has. Hidden when
// neither knows any.
//
// With workspace icons on (Look > Workspace icons; shell.json
// "workspaceIcons"), a workspace in use shows the icon of its first window
// instead of a dot, the active one on an accent pill (an idea from
// end-4/dots-hyprland, see CREDITS.md). Hyprland only; elsewhere the dots stay.
Item {
  id: root

  required property ShellScreen screen
  // Stacked top to bottom (the rail style).
  property bool vertical: false
  readonly property bool hyprland: !!Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE")

  // Not Hyprland: the compositor's workspaces for this screen, as it orders them.
  readonly property var wsets: {
    if (hyprland)
      return [];
    const proj = WindowManager.screenProjection(root.screen);
    const list = (proj ? proj.windowsets : WindowManager.windowsets) || [];
    return Array.from(list).filter(w => w && w.shouldDisplay);
  }
  readonly property HyprlandMonitor monitor: hyprland ? Hyprland.monitorFor(screen) : null
  readonly property int activeId: monitor && monitor.activeWorkspace ? monitor.activeWorkspace.id : 1

  readonly property var occupied: {
    const ids = {};
    for (const w of Hyprland.workspaces.values)
      if (w.id > 0 && (!w.monitor || !monitor || w.monitor === monitor))
        ids[w.id] = true;
    return ids;
  }
  readonly property bool icons: Config.workspaceIcons && hyprland
  // A workspace's first window's app id, from Hyprland's client list.
  readonly property var firstApp: {
    const out = {};
    if (!icons)
      return out;
    for (const t of Hyprland.toplevels.values) {
      const ws = t.workspace;
      const cls = t.lastIpcObject ? t.lastIpcObject.class : "";
      if (ws && ws.id > 0 && cls && out[ws.id] === undefined)
        out[ws.id] = cls;
    }
    return out;
  }
  readonly property int slots: {
    let n = Config.workspaces;
    for (const id in occupied)
      n = Math.max(n, Number(id));
    return Math.max(n, activeId);
  }

  // The active slot, and where it sits in this item (the blade's hairline
  // follows it).
  property Item activeSlot: null
  readonly property real activeX: activeSlot ? row.x + activeSlot.x : 0
  readonly property real activeW: activeSlot ? activeSlot.width : 0

  visible: hyprland || wsets.length > 0
  implicitWidth: vertical ? 24 : row.implicitWidth + 16
  implicitHeight: vertical ? row.implicitHeight + 16 : 24

  function go(target) {
    Quickshell.execDetached(["hyprctl", "dispatch", "hl.dsp.focus({ workspace = \"" + target + "\" })"]);
  }

  // Not Hyprland: step to the neighbouring workspace.
  function step(by) {
    const i = wsets.findIndex(w => w.active);
    const next = wsets[Math.max(0, Math.min(wsets.length - 1, i + by))];
    if (next && next.canActivate)
      next.activate();
  }

  Rectangle {
    anchors.fill: parent
    radius: height / 2
    color: Theme.surface
  }

  GridLayout {
    id: row
    anchors.centerIn: parent
    flow: root.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
    rows: root.vertical ? -1 : 1
    columns: root.vertical ? 1 : -1
    rowSpacing: 5
    columnSpacing: 5

    Repeater {
      model: root.hyprland ? root.slots : root.wsets.length
      Item {
        id: slot
        required property int index
        readonly property int wsId: index + 1
        readonly property var wset: root.hyprland ? null : root.wsets[index]
        readonly property bool active: root.hyprland ? wsId === root.activeId : !!(wset && wset.active)
        // The compositor lists only workspaces that exist, so all are in use.
        readonly property bool used: root.hyprland ? root.occupied[wsId] === true : true

        readonly property string app: root.firstApp[wsId] || ""
        readonly property bool showIcon: root.icons && app !== ""
        readonly property real dotW: showIcon ? (active ? 26 : 18) : active ? 22 : 8
        readonly property real dotH: showIcon ? 18 : 8
        implicitWidth: root.vertical ? (showIcon ? 18 : 8) : dotW
        implicitHeight: root.vertical ? (showIcon ? (active ? 26 : 18) : active ? 22 : 8) : dotH
        Behavior on implicitWidth { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
        Behavior on implicitHeight { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
        Binding { when: slot.active; target: root; property: "activeSlot"; value: slot; restoreMode: Binding.RestoreNone }

        Rectangle {
          anchors.fill: parent
          radius: Math.min(width, height) / 2
          color: slot.active ? Theme.accent
            : slot.showIcon ? "transparent"
            : slot.used ? Theme.subtext : Theme.overlay
          Behavior on color { ColorAnimation { duration: Theme.medium } }
        }
        Image {
          visible: slot.showIcon
          anchors.centerIn: parent
          width: 14; height: 14
          sourceSize: Qt.size(28, 28)
          asynchronous: true
          source: slot.showIcon ? AppIcons.source((DesktopEntries.heuristicLookup(slot.app) || {}).icon || slot.app) : ""
          opacity: slot.active ? 1 : 0.75
        }
        MouseArea {
          anchors.fill: parent
          anchors.margins: -4
          cursorShape: Qt.PointingHandCursor
          onClicked: {
            if (root.hyprland)
              root.go(String(slot.wsId));
            else if (slot.wset && slot.wset.canActivate)
              slot.wset.activate();
          }
        }
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.NoButton
    onWheel: w => root.hyprland ? root.go(w.angleDelta.y > 0 ? "e-1" : "e+1") : root.step(w.angleDelta.y > 0 ? -1 : 1)
  }
}
