import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import qs.common

// Rounded screen corners (switch: screen-corners): four small black fillets
// over the screen's corners, so the desktop, the bar and full-screen windows
// all meet the bezel in a curve. One click-through surface over the screen,
// not one per corner: each surface FRIDAY Shell keeps up costs memory of its
// own (about 60 MB each under software rendering), far more than its pixels.
LazyLoader {
  id: root

  required property ShellScreen modelData
  readonly property int size: 14

  active: Config.featureOn("screen-corners")

  PanelWindow {
    screen: root.modelData
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    // Takes no input anywhere.
    mask: Region {}
    WlrLayershell.namespace: "friday-corner"
    WlrLayershell.layer: WlrLayer.Overlay

    Repeater {
      model: [0, 1, 2, 3]
      // The area between a corner point and a quarter circle.
      Shape {
        id: corner
        required property int modelData
        readonly property bool atLeft: modelData === 0 || modelData === 2
        readonly property bool atTop: modelData < 2
        x: atLeft ? 0 : parent.width - root.size
        y: atTop ? 0 : parent.height - root.size
        width: root.size
        height: root.size
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
          fillColor: "black"
          strokeWidth: -1
          startX: corner.atLeft ? 0 : root.size
          startY: corner.atTop ? 0 : root.size
          PathLine { x: corner.atLeft ? root.size : 0; y: corner.atTop ? 0 : root.size }
          PathArc {
            x: corner.atLeft ? 0 : root.size
            y: corner.atTop ? root.size : 0
            radiusX: root.size
            radiusY: root.size
            direction: corner.atLeft === corner.atTop ? PathArc.Counterclockwise : PathArc.Clockwise
          }
          PathLine { x: corner.atLeft ? 0 : root.size; y: corner.atTop ? 0 : root.size }
        }
      }
    }
  }
}
