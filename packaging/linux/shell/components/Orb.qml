import QtQuick
import QtQuick.Shapes
import qs.common
import qs.services

// FRIDAY's mark: a core inside a ring, and the one place the shell shows
// what FRIDAY is doing at a glance.
//   ready    ember core, breathing slowly
//   working  two arcs orbit the core (ember one way, signal the other)
//   stopped  amber core, still
//   offline  an empty ring
Item {
  id: root

  property real size: 22
  property string mode: Friday.mode

  implicitWidth: size
  implicitHeight: size

  readonly property color coreColor: mode === "stopped" ? Theme.warn
    : mode === "offline" ? "transparent" : Theme.accent

  // Ring.
  Rectangle {
    anchors.centerIn: parent
    width: root.size
    height: root.size
    radius: width / 2
    color: "transparent"
    border.width: Math.max(1.5, root.size / 14)
    border.color: root.mode === "offline" ? Theme.faint : Theme.alpha(Theme.accent, 0.35)
  }

  // Halo that breathes while ready.
  Rectangle {
    id: halo
    anchors.centerIn: parent
    width: root.size * 0.62
    height: width
    radius: width / 2
    color: Theme.alpha(Theme.accent, 0.35)
    visible: root.mode === "ready"
    SequentialAnimation on scale {
      running: halo.visible
      loops: Animation.Infinite
      NumberAnimation { from: 0.8; to: 1.35; duration: 1600; easing.type: Easing.InOutSine }
      NumberAnimation { from: 1.35; to: 0.8; duration: 1600; easing.type: Easing.InOutSine }
    }
  }

  // Core.
  Rectangle {
    anchors.centerIn: parent
    width: root.size * (root.mode === "working" ? 0.32 : 0.4)
    height: width
    radius: width / 2
    color: root.coreColor
    Behavior on width { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
  }

  // Orbits while working.
  Repeater {
    model: root.mode === "working" ? 2 : 0
    Shape {
      required property int index
      anchors.fill: parent
      preferredRendererType: Shape.CurveRenderer
      ShapePath {
        strokeColor: index === 0 ? Theme.accent : Theme.signal
        strokeWidth: Math.max(1.5, root.size / 11)
        fillColor: "transparent"
        capStyle: ShapePath.RoundCap
        PathAngleArc {
          centerX: root.size / 2
          centerY: root.size / 2
          radiusX: root.size / 2 - Math.max(1.5, root.size / 14)
          radiusY: radiusX
          startAngle: index === 0 ? -90 : 90
          sweepAngle: index === 0 ? 110 : 70
        }
      }
      RotationAnimation on rotation {
        loops: Animation.Infinite
        from: index === 0 ? 0 : 360
        to: index === 0 ? 360 : 0
        duration: index === 0 ? 1100 : 1700
      }
    }
  }
}
