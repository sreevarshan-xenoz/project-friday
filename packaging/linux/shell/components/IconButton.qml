import QtQuick
import Quickshell
import qs.common

// A round icon button. `active` fills it with the accent. Its `tooltip`
// shows in a small label after the pointer rests on it for 600 ms (the
// label's window exists only while it shows).
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
  // What a screen reader says, and pressing it from one.
  Accessible.role: Accessible.Button
  Accessible.name: root.tooltip
  Accessible.onPressAction: root.clicked()
  Keys.onReturnPressed: root.clicked()
  Keys.onEnterPressed: root.clicked()
  Keys.onSpacePressed: root.clicked()

  Rectangle {
    anchors.fill: parent
    radius: Theme.round(height)
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
    onClicked: { tipDelay.stop(); root.tipShown = false; root.clicked(); }
    onContainsMouseChanged: { root.tipShown = false; if (containsMouse && root.tooltip !== "") tipDelay.restart(); else tipDelay.stop(); }
  }

  property bool tipShown: false
  Timer { id: tipDelay; interval: 600; onTriggered: root.tipShown = mouse.containsMouse }

  Loader {
    active: root.tipShown && root.visible
    sourceComponent: PopupWindow {
      anchor.item: root
      // Beside the rail, not over the buttons below it.
      anchor.edges: Theme.rail ? Edges.Right : Theme.barAtBottom ? Edges.Top : Edges.Bottom
      anchor.gravity: Theme.rail ? Edges.Right : Theme.barAtBottom ? Edges.Top : Edges.Bottom
      anchor.margins.left: Theme.rail ? 8 : 0
      anchor.margins.top: Theme.barAtBottom || Theme.rail ? 0 : 6
      anchor.margins.bottom: Theme.barAtBottom ? 6 : 0
      implicitWidth: tipText.implicitWidth + 20
      implicitHeight: tipText.implicitHeight + 10
      color: "transparent"
      visible: true
      Rectangle {
        anchors.fill: parent
        radius: Theme.round(height)
        color: Theme.glass
        border.width: 1
        border.color: Theme.glassEdge
        Label {
          id: tipText
          anchors.centerIn: parent
          text: root.tooltip
          font.pixelSize: Theme.fontS + 1
        }
      }
    }
  }
}
