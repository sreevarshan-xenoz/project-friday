import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components
import qs.services

// CPU and memory in use, as two small numbers in the bar (switch:
// bar-resources); either turns the warning colour when it runs hot. An idea
// from end-4/dots-hyprland's bar resources (see CREDITS.md).
Item {
  id: root

  visible: Resources.on
  implicitWidth: row.implicitWidth
  implicitHeight: 24

  RowLayout {
    id: row
    anchors.verticalCenter: parent.verticalCenter
    spacing: 8
    component Reading: RowLayout {
      property string glyph
      property real value
      property bool hot
      spacing: 3
      Glyph { text: parent.glyph; size: 13; color: parent.hot ? Theme.warn : Theme.faint }
      Label {
        text: Math.round(parent.value * 100) + "%"
        font.family: Theme.mono
        font.pixelSize: Theme.fontS
        color: parent.hot ? Theme.warn : Theme.subtext
      }
    }
    Reading { glyph: Icons.cpu; value: Resources.cpu; hot: Resources.cpuHot }
    Reading { glyph: Icons.memory; value: Resources.mem; hot: Resources.memHot }
  }
}
