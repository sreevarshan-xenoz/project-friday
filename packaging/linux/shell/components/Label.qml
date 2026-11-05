import QtQuick
import qs.common

// Body text in the shell's type scale.
Text {
  color: Theme.text
  font.family: Theme.sans
  font.pixelSize: Theme.fontM
  elide: Text.ElideRight
  verticalAlignment: Text.AlignVCenter
  renderType: Text.NativeRendering
}
