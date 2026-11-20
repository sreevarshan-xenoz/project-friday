import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The "Show me" ring (services/Ring.qml) on this screen, if the spot is on
// it. The whole layer lets clicks through: the ring only points.
PanelWindow {
  id: overlay

  required property ShellScreen modelData
  screen: modelData

  // The spot in this screen's own coordinates.
  readonly property int rx: Ring.x - modelData.x
  readonly property int ry: Ring.y - modelData.y
  readonly property bool here: rx + Ring.width > 0 && ry + Ring.height > 0
                               && rx < modelData.width && ry < modelData.height

  visible: (Ring.shown && here) || ring.opacity > 0.01
  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-ring"
  WlrLayershell.layer: WlrLayer.Overlay
  mask: Region {}

  Rectangle {
    id: ring
    readonly property int pad: 6
    x: overlay.rx - pad
    y: overlay.ry - pad
    width: Ring.width + pad * 2
    height: Ring.height + pad * 2
    radius: Theme.radiusS + pad
    color: "transparent"
    border.width: 3
    border.color: Theme.accent
    opacity: Ring.shown && overlay.here ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.medium } }

    // A slow pulse, so the eye finds it.
    SequentialAnimation on scale {
      running: ring.opacity > 0
      loops: Animation.Infinite
      NumberAnimation { from: 1.0; to: 1.08; duration: 650; easing.type: Easing.InOutQuad }
      NumberAnimation { from: 1.08; to: 1.0; duration: 650; easing.type: Easing.InOutQuad }
    }
  }

  // The label sits under the ring (above it near the bottom edge).
  Rectangle {
    visible: Ring.label !== ""
    opacity: ring.opacity
    readonly property bool above: ring.y + ring.height + height + 8 > overlay.height
    x: Math.max(4, Math.min(ring.x, overlay.width - width - 4))
    y: above ? ring.y - height - 8 : ring.y + ring.height + 8
    width: text.implicitWidth + 20
    height: text.implicitHeight + 10
    radius: height / 2
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
    Label {
      id: text
      anchors.centerIn: parent
      text: Ring.label
    }
  }
}
