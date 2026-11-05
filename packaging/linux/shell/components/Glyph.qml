import QtQuick
import qs.common

// One icon glyph (see common/Icons.qml).
Text {
  property real size: 18
  color: Theme.text
  font.family: Theme.mono
  font.pixelSize: size
  horizontalAlignment: Text.AlignHCenter
  verticalAlignment: Text.AlignVCenter
  renderType: Text.NativeRendering
}
