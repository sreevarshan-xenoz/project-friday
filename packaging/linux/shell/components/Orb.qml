import QtQuick
import QtQuick.Shapes
import qs.common
import qs.services

// FRIDAY's mark, the aperture F, and the one place the shell shows what
// FRIDAY is doing at a glance. Each mode is one of the logo's states, in the
// palette's colours (so every palette has its own mark):
//   ready      idle: silver blades, platinum F, still
//   listening  scanning: an accent ring of ticks turning round dim blades
//   working    thinking: dim blades turning, an accent dot at the centre
//              and an arc going round
//   speaking   active: accent blades, the F lit, a glow behind it
//   stopped    the idle mark in amber
//   offline    the idle mark, dim
// The shapes are drawn on a 100 x 100 box and scaled to size.
Item {
  id: root

  property real size: 22
  property string mode: Friday.mode

  implicitWidth: size
  implicitHeight: size

  readonly property string blades: "M50.00 6.00 A44 44 0 0 1 78.28 16.29 Q81.38 39.80 66.74 52.95 Q71.28 33.38 50.00 6.00 Z M88.11 28.00 A44 44 0 0 1 93.33 57.64 Q74.52 72.08 55.81 65.97 Q75.03 60.11 88.11 28.00 Z M88.11 72.00 A44 44 0 0 1 65.05 91.35 Q43.14 82.28 39.07 63.02 Q53.76 76.74 88.11 72.00 Z M50.00 94.00 A44 44 0 0 1 21.72 83.71 Q18.62 60.20 33.26 47.05 Q28.72 66.62 50.00 94.00 Z M11.89 72.00 A44 44 0 0 1 6.67 42.36 Q25.48 27.92 44.19 34.03 Q24.97 39.89 11.89 72.00 Z M11.89 28.00 A44 44 0 0 1 34.95 8.65 Q56.86 17.72 60.93 36.98 Q46.24 23.26 11.89 28.00 Z"
  readonly property string letter: "M44.40 24.00 L82.40 24.00 L71.70 33.00 L52.70 33.00 L48.40 44.00 L66.40 44.00 L57.00 52.00 L45.00 52.00 L36.40 74.00 L24.00 82.00 Z"

  readonly property bool offline: mode === "offline"
  readonly property bool scanning: mode === "listening"
  readonly property bool thinking: mode === "working"
  readonly property bool active: mode === "speaking"
  // The F shows when FRIDAY is at rest or answering; the busy states show
  // what is going on instead.
  readonly property bool showLetter: !scanning && !thinking

  readonly property color bladeColor: offline ? Theme.alpha(Theme.faint, 0.35)
    : mode === "stopped" ? Theme.warn
    : active ? Theme.accent
    : (scanning || thinking) ? Theme.alpha(Theme.subtext, 0.28)
    : Theme.subtext

  // Glow behind the mark while it answers.
  Rectangle {
    id: glow
    anchors.centerIn: parent
    width: root.size * 0.7
    height: width
    radius: width / 2
    color: Theme.alpha(Theme.accent, 0.45)
    visible: root.active
    SequentialAnimation on scale {
      running: glow.visible && Theme.motion
      loops: Animation.Infinite
      NumberAnimation { from: 0.85; to: 1.25; duration: 900; easing.type: Easing.InOutSine }
      NumberAnimation { from: 1.25; to: 0.85; duration: 900; easing.type: Easing.InOutSine }
    }
  }

  // The 100 x 100 drawing, scaled.
  Item {
    width: 100
    height: 100
    scale: root.size / 100
    transformOrigin: Item.TopLeft

    // The blades turn slowly while FRIDAY is busy.
    Shape {
      id: bladeShape
      anchors.fill: parent
      preferredRendererType: Shape.CurveRenderer
      ShapePath {
        fillColor: root.bladeColor
        strokeColor: "transparent"
        PathSvg { path: root.blades }
      }
      RotationAnimation on rotation {
        running: (root.scanning || root.thinking) && Theme.motion
        loops: Animation.Infinite
        from: 0
        to: 360
        duration: root.thinking ? 2400 : 6000
      }
      Binding on rotation { when: !(root.scanning || root.thinking) || !Theme.motion; value: 0 }
    }

    // The F, cut out from the blades by a band of the background round it.
    Shape {
      anchors.fill: parent
      visible: root.showLetter
      preferredRendererType: Shape.CurveRenderer
      ShapePath {
        fillColor: Theme.bg
        strokeColor: Theme.bg
        strokeWidth: 13
        joinStyle: ShapePath.MiterJoin
        PathSvg { path: root.letter }
      }
      ShapePath {
        fillColor: root.offline ? Theme.faint : Theme.text
        strokeColor: "transparent"
        PathSvg { path: root.letter }
      }
    }

    // Scanning: a ring of ticks in the accent, turning.
    Item {
      id: ticks
      anchors.fill: parent
      visible: root.scanning
      Repeater {
        model: 12
        Item {
          required property int index
          anchors.fill: parent
          rotation: index * 30
          Rectangle {
            x: 50 - width / 2
            y: 1
            width: index % 3 === 0 ? 6 : 4
            height: index % 3 === 0 ? 14 : 9
            radius: 2
            color: Theme.accent
          }
        }
      }
      RotationAnimation on rotation {
        running: ticks.visible && Theme.motion
        loops: Animation.Infinite
        from: 0
        to: 360
        duration: 4000
      }
    }

    // Thinking: an accent dot at the centre and an arc going round.
    Rectangle {
      id: dot
      visible: root.thinking
      x: 50 - width / 2
      y: 50 - height / 2
      width: 22
      height: 22
      radius: 11
      color: Theme.accent
      SequentialAnimation on opacity {
        running: dot.visible && Theme.motion
        loops: Animation.Infinite
        NumberAnimation { from: 1; to: 0.45; duration: 700; easing.type: Easing.InOutSine }
        NumberAnimation { from: 0.45; to: 1; duration: 700; easing.type: Easing.InOutSine }
      }
    }
    Shape {
      anchors.fill: parent
      visible: root.thinking
      preferredRendererType: Shape.CurveRenderer
      ShapePath {
        strokeColor: Theme.accent
        strokeWidth: 8
        fillColor: "transparent"
        capStyle: ShapePath.RoundCap
        PathAngleArc { centerX: 50; centerY: 50; radiusX: 45; radiusY: 45; startAngle: -90; sweepAngle: 100 }
      }
      RotationAnimation on rotation {
        running: root.thinking && Theme.motion
        loops: Animation.Infinite
        from: 0
        to: 360
        duration: 1100
      }
    }
  }
}
