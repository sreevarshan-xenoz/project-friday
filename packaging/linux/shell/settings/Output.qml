import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

// What a command printed, in a scrollable box.
Rectangle {
  id: root

  property string text: ""
  // Longer output scrolls past this height.
  property int maxHeight: 190

  Layout.fillWidth: true
  // As tall as the text, up to maxHeight; longer output scrolls.
  Layout.preferredHeight: Math.min(root.maxHeight, body.implicitHeight + 20)
  radius: Theme.radiusS
  color: Theme.surface
  visible: root.text !== ""
  clip: true

  Flickable {
    anchors.fill: parent
    anchors.margins: 10
    contentHeight: body.implicitHeight
    boundsBehavior: Flickable.StopAtBounds
    Text {
      id: body
      width: parent.width
      text: root.text
      wrapMode: Text.Wrap
      color: Theme.subtext
      font.family: Theme.mono
      font.pixelSize: Theme.fontS + 1
    }
  }
}
