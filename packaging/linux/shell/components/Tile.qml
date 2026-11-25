import QtQuick
import QtQuick.Layouts
import qs.common

// A quick-settings toggle: glyph, title and a status line. Lit when `on`.
Rectangle {
  id: root

  property string icon: ""
  property string title: ""
  property string status: ""
  property bool on: false
  signal clicked()
  signal secondaryClicked()

  implicitHeight: 56
  radius: Theme.radiusS + 4
  color: on ? Theme.alpha(Theme.accent, 0.16) : (mouse.containsMouse ? Theme.overlay : Theme.raised)
  border.width: activeFocus ? 2 : 1
  border.color: activeFocus ? Theme.text : on ? Theme.alpha(Theme.accent, 0.55) : "transparent"
  activeFocusOnTab: true
  // What a screen reader says, and pressing it from one.
  Accessible.role: Accessible.Button
  Accessible.checkable: true
  Accessible.checked: root.on
  Accessible.name: root.title
  Accessible.description: root.status
  Accessible.onPressAction: root.clicked()
  Keys.onReturnPressed: root.clicked()
  Keys.onEnterPressed: root.clicked()
  Keys.onSpacePressed: root.clicked()
  Keys.onMenuPressed: root.secondaryClicked()
  Behavior on color { ColorAnimation { duration: Theme.fast } }

  RowLayout {
    anchors.fill: parent
    anchors.leftMargin: 14
    anchors.rightMargin: 12
    spacing: 10
    Glyph {
      text: root.icon
      size: 20
      color: root.on ? Theme.accent : Theme.subtext
    }
    ColumnLayout {
      Layout.fillWidth: true
      spacing: 0
      Label {
        Layout.fillWidth: true
        text: root.title
        font.weight: Font.DemiBold
      }
      Label {
        Layout.fillWidth: true
        visible: text !== ""
        text: root.status
        font.pixelSize: Theme.fontS
        color: Theme.subtext
      }
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.LeftButton | Qt.RightButton
    cursorShape: Qt.PointingHandCursor
    onClicked: m => m.button === Qt.RightButton ? root.secondaryClicked() : root.clicked()
  }
}
