import QtQuick
import QtQuick.Shapes
import qs.common

// One concave corner under the bar at a screen edge, so the bar flows into
// the screen instead of ending in a hard line. `leftSide` picks the side.
Shape {
  id: root

  property bool leftSide: true
  property color color: Theme.glass
  property real size: Theme.fillet

  width: size
  height: size
  preferredRendererType: Shape.CurveRenderer

  ShapePath {
    fillColor: root.color
    strokeWidth: -1
    startX: root.leftSide ? 0 : root.size
    startY: 0
    PathLine { x: root.leftSide ? root.size : 0; y: 0 }
    PathArc {
      x: root.leftSide ? 0 : root.size
      y: root.size
      radiusX: root.size
      radiusY: root.size
      direction: root.leftSide ? PathArc.Counterclockwise : PathArc.Clockwise
    }
    PathLine { x: root.leftSide ? 0 : root.size; y: 0 }
  }
}
