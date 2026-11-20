import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The live captions (services/Captions.qml): a bar low on the focused
// screen. Clicks pass through it.
PanelWindow {
  id: overlay

  required property ShellScreen modelData
  screen: modelData

  readonly property bool here: Shell.focusedScreen() === modelData.name
  visible: (Captions.shown && here) || bar.opacity > 0.01

  anchors { bottom: true; left: true; right: true }
  margins.bottom: 96 + (Theme.barAtBottom ? Theme.barReach : 0)
  implicitHeight: bar.height
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-captions"
  WlrLayershell.layer: WlrLayer.Overlay
  mask: Region {}

  Rectangle {
    id: bar
    anchors.horizontalCenter: parent.horizontalCenter
    width: Math.min(parent.width * 0.7, words.implicitWidth + 40)
    height: words.implicitHeight + 20
    radius: Theme.radiusS
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
    opacity: Captions.shown && overlay.here ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.medium } }

    Label {
      id: words
      anchors.centerIn: parent
      width: Math.min(overlay.width * 0.7 - 40, implicitWidth)
      text: Captions.text
      wrapMode: Text.Wrap
      horizontalAlignment: Text.AlignHCenter
      font.pixelSize: Theme.fontM + 3
    }
  }
}
