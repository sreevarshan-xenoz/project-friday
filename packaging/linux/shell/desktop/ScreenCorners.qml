import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import qs.common

// Rounded screen corners (switch: screen-corners): four small black
// fillets over the screen's corners, so the desktop, the bar and full-screen
// windows all meet the bezel in a curve. Four tiny surfaces that never
// redraw and let every click through.
Scope {
  id: root

  required property ShellScreen modelData
  readonly property int size: 14

  Variants {
    model: Config.featureOn("screen-corners") ? [0, 1, 2, 3] : []

    PanelWindow {
      id: corner
      required property int modelData
      readonly property bool left: modelData === 0 || modelData === 2
      readonly property bool top: modelData < 2

      screen: root.modelData
      anchors { top: corner.top; bottom: !corner.top; left: corner.left; right: !corner.left }
      implicitWidth: root.size
      implicitHeight: root.size
      exclusionMode: ExclusionMode.Ignore
      color: "transparent"
      mask: Region {}
      WlrLayershell.namespace: "friday-corner"
      WlrLayershell.layer: WlrLayer.Overlay

      // The area between the corner point and a quarter circle.
      Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
          fillColor: "black"
          strokeWidth: -1
          readonly property real s: root.size
          readonly property real cx: corner.left ? 0 : s
          readonly property real cy: corner.top ? 0 : s
          startX: cx
          startY: cy
          PathLine { x: corner.left ? root.size : 0; y: corner.top ? 0 : root.size }
          PathArc {
            x: corner.left ? 0 : root.size
            y: corner.top ? root.size : 0
            radiusX: root.size
            radiusY: root.size
            direction: corner.left === corner.top ? PathArc.Counterclockwise : PathArc.Clockwise
          }
          PathLine { x: corner.left ? 0 : root.size; y: corner.top ? 0 : root.size }
        }
      }
    }
  }
}
