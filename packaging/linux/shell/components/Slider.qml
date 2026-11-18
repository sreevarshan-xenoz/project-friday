import QtQuick
import qs.common

// A thick rounded slider with the glyph inside the track (drag or scroll).
Item {
  id: root

  property real value: 0
  property string icon: ""
  property color fill: Theme.accent
  signal moved(real value)
  signal iconClicked()

  implicitHeight: 40
  implicitWidth: 240
  activeFocusOnTab: true
  Keys.onLeftPressed: root.moved(Math.max(0, root.value - 0.05))
  Keys.onRightPressed: root.moved(Math.min(1, root.value + 0.05))

  Rectangle {
    id: track
    anchors.fill: parent
    radius: height / 2
    color: Theme.raised
    border.width: root.activeFocus ? 2 : 0
    border.color: Theme.text
    clip: true

    Rectangle {
      height: parent.height
      width: Math.max(parent.height, parent.width * root.value)
      radius: height / 2
      color: root.fill
      Behavior on width { enabled: !drag.pressed; NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
    }
  }

  Glyph {
    x: (root.height - width) / 2
    anchors.verticalCenter: parent.verticalCenter
    width: 24
    text: root.icon
    size: 18
    color: Theme.onAccent
    z: 2
    MouseArea {
      anchors.fill: parent
      anchors.margins: -6
      cursorShape: Qt.PointingHandCursor
      onClicked: root.iconClicked()
    }
  }

  Label {
    anchors.right: parent.right
    anchors.rightMargin: 14
    anchors.verticalCenter: parent.verticalCenter
    text: Math.round(root.value * 100) + "%"
    font.family: Theme.mono
    font.pixelSize: Theme.fontS
    color: Theme.subtext
  }

  MouseArea {
    id: drag
    anchors.fill: parent
    anchors.leftMargin: root.height
    cursorShape: Qt.PointingHandCursor
    function set(mx) { root.moved(Math.max(0, Math.min(1, (mx + root.height) / root.width))); }
    onPressed: m => set(m.x)
    onPositionChanged: m => { if (pressed) set(m.x); }
    onWheel: w => root.moved(Math.max(0, Math.min(1, root.value + (w.angleDelta.y > 0 ? 0.05 : -0.05))))
  }
}
