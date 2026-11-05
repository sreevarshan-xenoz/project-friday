import QtQuick
import QtQuick.Shapes
import qs.common

// The outline every drawer and the bar's screen corners share: a body that
// hangs from the top edge, joined to it by two concave fillets, with rounded
// bottom corners. Painted in the bar's colour, it reads as the bar itself
// growing downward.
//
//      ___________________________________   <- top edge (joins the bar)
//      \_                               _/   <- concave fillets
//        |                             |
//        |            body             |
//         \___________________________/      <- rounded bottom
Shape {
  id: root

  property color color: Theme.bg
  property real fillet: Theme.fillet
  property real radius: Theme.radius

  // Clamped so the outline stays valid while a drawer animates from height 0.
  readonly property real r: Math.max(0, Math.min(fillet, height, (width) / 4))
  readonly property real br: Math.max(0, Math.min(radius, (height - r) / 2, (width - 2 * r) / 2))
  readonly property real w: width
  readonly property real h: height

  preferredRendererType: Shape.CurveRenderer

  ShapePath {
    fillColor: root.color
    strokeWidth: -1
    startX: 0
    startY: 0
    PathLine { x: root.w; y: 0 }
    PathArc { x: root.w - root.r; y: root.r; radiusX: root.r; radiusY: root.r; direction: PathArc.Counterclockwise }
    PathLine { x: root.w - root.r; y: root.h - root.br }
    PathArc { x: root.w - root.r - root.br; y: root.h; radiusX: root.br; radiusY: root.br; direction: PathArc.Clockwise }
    PathLine { x: root.r + root.br; y: root.h }
    PathArc { x: root.r; y: root.h - root.br; radiusX: root.br; radiusY: root.br; direction: PathArc.Clockwise }
    PathLine { x: root.r; y: root.r }
    PathArc { x: 0; y: 0; radiusX: root.r; radiusY: root.r; direction: PathArc.Counterclockwise }
  }
}
