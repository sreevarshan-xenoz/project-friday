import QtQuick
import QtQuick.Shapes
import qs.common

// A desktop widget's card: frosted, it leans toward the pointer with a soft
// light following it, and can be dragged anywhere (the spot is kept in
// shell.json under "widgets"). Nothing redraws unless the pointer is on it
// or it is being moved.
Rectangle {
  id: card

  // Its name in shell.json, and where it starts (fractions of the screen).
  required property string key
  property real homeX: 0.05
  property real homeY: 0.6
  // The screen area it lives in.
  property real areaW: parent ? parent.width : 0
  property real areaH: parent ? parent.height : 0

  readonly property var saved: (Config.settings.widgets || {})[key] || null
  x: Math.round((saved ? saved.x : homeX) * Math.max(1, areaW - width))
  y: Math.round((saved ? saved.y : homeY) * Math.max(1, areaH - height))

  radius: 22
  color: Theme.glassSolid
  border.width: 1
  border.color: hover.hovered ? Theme.alpha(Theme.accent, 0.45) : Theme.glassEdge
  Behavior on border.color { ColorAnimation { duration: Theme.medium } }

  // Where the pointer is on the card, 0..1 each way.
  readonly property real px: hover.hovered ? hover.point.position.x / Math.max(1, width) : 0.5
  readonly property real py: hover.hovered ? hover.point.position.y / Math.max(1, height) : 0.5
  readonly property bool lively: hover.hovered && Theme.motion && !drag.active

  HoverHandler { id: hover }

  // Lean toward the pointer, up to 7 degrees, and lift a little.
  property real lean: lively ? 7 : 0
  Behavior on lean { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
  scale: drag.active ? 1.04 : lively ? 1.015 : 1
  Behavior on scale { NumberAnimation { duration: Theme.medium; easing.type: Theme.spring } }
  transform: Rotation {
    origin.x: card.width / 2
    origin.y: card.height / 2
    axis { x: -(card.py - 0.5); y: card.px - 0.5; z: 0 }
    angle: card.lean
  }

  // A soft light that follows the pointer.
  Shape {
    anchors.fill: parent
    opacity: card.lively ? 1 : 0
    visible: opacity > 0.01
    Behavior on opacity { NumberAnimation { duration: Theme.medium } }
    layer.enabled: false
    ShapePath {
      strokeWidth: 0
      strokeColor: "transparent"
      fillGradient: RadialGradient {
        centerX: card.px * card.width; centerY: card.py * card.height
        focalX: centerX; focalY: centerY
        centerRadius: card.width * 0.6
        GradientStop { position: 0; color: Theme.alpha(Theme.accent, 0.16) }
        GradientStop { position: 1; color: "transparent" }
      }
      PathRectangle { x: 0; y: 0; width: card.width; height: card.height; radius: card.radius }
    }
  }

  // Drag it anywhere; let go and it stays there.
  DragHandler {
    id: drag
    target: card
    xAxis.minimum: 0; xAxis.maximum: card.areaW - card.width
    yAxis.minimum: 0; yAxis.maximum: card.areaH - card.height
    onActiveChanged: {
      if (active) return;
      const all = Object.assign({}, Config.settings.widgets || {});
      all[card.key] = {
        x: Math.round(1000 * card.x / Math.max(1, card.areaW - card.width)) / 1000,
        y: Math.round(1000 * card.y / Math.max(1, card.areaH - card.height)) / 1000
      };
      Config.set("widgets", all);
    }
  }
}
