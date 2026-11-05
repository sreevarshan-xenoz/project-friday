import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import qs.common
import qs.components

// Workspace slots for this bar's monitor: the active one is a wide ember
// bar, occupied ones a solid dot, empty ones a faint dot. Click to go there,
// scroll to step. Hyprland only; elsewhere the row is hidden.
Item {
  id: root

  required property ShellScreen screen
  readonly property bool hyprland: !!Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE")
  readonly property HyprlandMonitor monitor: hyprland ? Hyprland.monitorFor(screen) : null
  readonly property int activeId: monitor && monitor.activeWorkspace ? monitor.activeWorkspace.id : 1

  readonly property var occupied: {
    const ids = {};
    for (const w of Hyprland.workspaces.values)
      if (w.id > 0 && (!w.monitor || !monitor || w.monitor === monitor))
        ids[w.id] = true;
    return ids;
  }
  readonly property int slots: {
    let n = Config.workspaces;
    for (const id in occupied)
      n = Math.max(n, Number(id));
    return Math.max(n, activeId);
  }

  visible: hyprland
  implicitWidth: row.implicitWidth + 16
  implicitHeight: 24

  function go(target) {
    Quickshell.execDetached(["hyprctl", "dispatch", "hl.dsp.focus({ workspace = \"" + target + "\" })"]);
  }

  Rectangle {
    anchors.fill: parent
    radius: height / 2
    color: Theme.surface
  }

  RowLayout {
    id: row
    anchors.centerIn: parent
    spacing: 5

    Repeater {
      model: root.slots
      Item {
        id: slot
        required property int index
        readonly property int wsId: index + 1
        readonly property bool active: wsId === root.activeId
        readonly property bool used: root.occupied[wsId] === true

        implicitWidth: active ? 22 : 8
        implicitHeight: 8
        Behavior on implicitWidth { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

        Rectangle {
          anchors.fill: parent
          radius: height / 2
          color: slot.active ? Theme.accent : slot.used ? Theme.subtext : Theme.overlay
          Behavior on color { ColorAnimation { duration: Theme.medium } }
        }
        MouseArea {
          anchors.fill: parent
          anchors.margins: -4
          cursorShape: Qt.PointingHandCursor
          onClicked: root.go(String(slot.wsId))
        }
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.NoButton
    onWheel: w => root.go(w.angleDelta.y > 0 ? "e-1" : "e+1")
  }
}
