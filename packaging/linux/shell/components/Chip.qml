import QtQuick
import QtQuick.Layouts
import qs.common

// A pill-shaped button with an optional glyph and a label.
Rectangle {
  id: root

  property string icon: ""
  property string text: ""
  property bool primary: false
  signal clicked()

  implicitHeight: 32
  implicitWidth: row.implicitWidth + 24
  radius: height / 2
  color: primary ? (mouse.containsMouse ? Qt.lighter(Theme.accent, 1.08) : Theme.accent)
    : (mouse.containsMouse ? Theme.overlay : Theme.raised)
  Behavior on color { ColorAnimation { duration: Theme.fast } }
  border.width: activeFocus ? 2 : 0
  border.color: Theme.text
  activeFocusOnTab: true
  Keys.onReturnPressed: root.clicked()
  Keys.onEnterPressed: root.clicked()
  Keys.onSpacePressed: root.clicked()

  RowLayout {
    id: row
    anchors.centerIn: parent
    spacing: 6
    Glyph {
      visible: root.icon !== ""
      text: root.icon
      size: 15
      color: root.primary ? Theme.onAccent : Theme.text
    }
    Label {
      text: root.text
      font.pixelSize: Theme.fontS + 1
      font.weight: Font.Medium
      color: root.primary ? Theme.onAccent : Theme.text
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }
}
