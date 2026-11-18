import QtQuick
import qs.common

// A round icon button. `active` fills it with the accent.
Item {
  id: root

  property string icon: ""
  property real size: 32
  property real iconSize: size * 0.55
  property bool active: false
  property color tint: Theme.accent
  property string tooltip: ""
  signal clicked()

  implicitWidth: size
  implicitHeight: size
  activeFocusOnTab: true
  Keys.onReturnPressed: root.clicked()
  Keys.onEnterPressed: root.clicked()
  Keys.onSpacePressed: root.clicked()

  Rectangle {
    anchors.fill: parent
    radius: height / 2
    border.width: root.activeFocus ? 2 : 0
    border.color: Theme.text
    color: root.active ? root.tint
      : mouse.containsMouse ? Theme.overlay : "transparent"
    Behavior on color { ColorAnimation { duration: Theme.fast } }
  }

  Glyph {
    anchors.centerIn: parent
    text: root.icon
    size: root.iconSize
    color: root.active ? Theme.onAccent : Theme.text
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }
}
