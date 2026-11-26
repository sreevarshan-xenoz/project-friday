import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

// Lock, sleep, log out, restart, power off. Arrow keys move, Enter picks.
// Lock runs FRIDAY's lock screen directly (it doesn't depend on the idle
// daemon being up); without hyprlock, logind asks the session's own locker.
FocusScope {
  id: root

  signal done()

  readonly property var actions: [
    { icon: Icons.lock, label: "Lock", cmd: ["sh", "-c", "friday lock || loginctl lock-session"], danger: false },
    { icon: Icons.sleep, label: "Sleep", cmd: ["systemctl", "suspend"], danger: false },
    { icon: Icons.logout, label: "Log out", cmd: ["sh", "-c", "command -v uwsm >/dev/null && uwsm check is-active && exec uwsm stop; exec loginctl terminate-session \"$XDG_SESSION_ID\""], danger: false },
    { icon: Icons.restart, label: "Restart", cmd: ["systemctl", "reboot"], danger: true },
    { icon: Icons.power, label: "Power off", cmd: ["systemctl", "poweroff"], danger: true }
  ]
  property int selected: 0
  // Restart and Power off ask twice: the first press arms the row ("Press
  // again to …"), a second within 3 s runs it. Moving off it disarms.
  property int armed: -1
  onSelectedChanged: armed = -1
  Timer { id: disarm; interval: 3000; onTriggered: root.armed = -1 }

  implicitWidth: 240
  implicitHeight: column.implicitHeight
  focus: true

  Keys.onDownPressed: selected = Math.min(actions.length - 1, selected + 1)
  Keys.onUpPressed: selected = Math.max(0, selected - 1)
  Keys.onReturnPressed: run(selected)
  Keys.onEnterPressed: run(selected)

  function run(i) {
    if (actions[i].danger && armed !== i) {
      armed = i;
      disarm.restart();
      return;
    }
    root.done();
    Quickshell.execDetached(actions[i].cmd);
  }

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 4

    Label {
      Layout.bottomMargin: 6
      text: "Session"
      color: Theme.subtext
      font.pixelSize: Theme.fontS
      font.weight: Font.DemiBold
      font.letterSpacing: 1.5
    }

    Repeater {
      model: root.actions
      Rectangle {
        id: item
        required property var modelData
        required property int index
        readonly property bool sel: index === root.selected
        readonly property color hue: modelData.danger ? Theme.error : Theme.accent
        Layout.fillWidth: true
        implicitHeight: 44
        radius: Theme.radiusS + 2
        color: sel ? Theme.alpha(hue, 0.16) : "transparent"
        Behavior on color { ColorAnimation { duration: Theme.fast } }

        RowLayout {
          anchors.fill: parent
          anchors.leftMargin: 14
          spacing: 14
          // A fixed column, so every label starts at the same x.
          Glyph {
            Layout.preferredWidth: 24
            text: item.modelData.icon
            size: 19
            color: item.sel ? item.hue : item.modelData.danger ? Theme.alpha(Theme.error, 0.75) : Theme.subtext
          }
          Label {
            Layout.fillWidth: true
            text: root.armed === item.index ? "Press again to " + item.modelData.label.toLowerCase() : item.modelData.label
            color: root.armed === item.index ? Theme.error : Theme.text
            font.weight: item.sel ? Font.DemiBold : Font.Normal
          }
        }
        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onEntered: root.selected = item.index
          onClicked: root.run(item.index)
        }
      }
    }
  }
}
