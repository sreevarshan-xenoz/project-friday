import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

// A switch whose state comes from a `friday ... status` command and which
// turns things on and off with the matching commands. `onText` is what the
// status says when it is on.
ColumnLayout {
  id: root

  property string title: ""
  property string detail: ""
  property var statusArgs: []
  property var onArgs: []
  property var offArgs: []
  property string onText: ""
  property bool checked: false

  spacing: 4
  Layout.fillWidth: true

  function refresh() { status.run(); }
  function flip() {
    change.args = root.checked ? root.offArgs : root.onArgs;
    change.run();
  }
  Component.onCompleted: refresh()

  Cmd {
    id: status
    args: root.statusArgs
    onFinished: output => root.checked = output.indexOf(root.onText) !== -1
  }
  Cmd {
    id: change
    onFinished: root.refresh()
  }

  RowLayout {
    Layout.fillWidth: true
    spacing: 12
    Rectangle {
      id: track
      implicitWidth: 40
      implicitHeight: 22
      radius: 11
      color: root.checked ? Theme.accent : Theme.overlay
      border.width: activeFocus ? 2 : 0
      border.color: Theme.text
      activeFocusOnTab: true
      Keys.onReturnPressed: root.flip()
      Keys.onEnterPressed: root.flip()
      Keys.onSpacePressed: root.flip()
      Behavior on color { ColorAnimation { duration: Theme.fast } }
      Rectangle {
        width: 16
        height: 16
        radius: 8
        y: 3
        x: root.checked ? parent.width - width - 3 : 3
        color: root.checked ? Theme.onAccent : Theme.text
        Behavior on x { NumberAnimation { duration: Theme.fast } }
      }
      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: root.flip()
      }
    }
    Label {
      Layout.fillWidth: true
      text: root.title
      font.weight: Font.DemiBold
    }
  }
  Label {
    Layout.fillWidth: true
    Layout.leftMargin: 52
    text: root.detail
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
    font.pixelSize: Theme.fontS + 1
  }
}
