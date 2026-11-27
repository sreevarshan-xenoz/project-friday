import QtQuick
import qs.common
import qs.components

// A glyph that slides open into a slider while the pointer is on it (the
// blade style): drag or click along it to set, scroll to step. `value` is
// 0..1; `moved(v)` asks for a new one. After saatvik333/niri-dotfiles and
// noctalia-shell's sliding modules (an idea, see CREDITS.md).
Item {
  id: root

  property string icon
  property real value: 0
  property string tooltip
  signal moved(real v)

  readonly property bool open: hover.hovered || drag.pressed
  property real reveal: open ? 1 : 0
  Behavior on reveal { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

  implicitWidth: 24 + 88 * reveal
  implicitHeight: 24
  clip: true
  Accessible.role: Accessible.Slider
  Accessible.name: root.tooltip

  HoverHandler { id: hover }


  Glyph {
    anchors.verticalCenter: parent.verticalCenter
    x: 4
    text: root.icon
    size: 15
    color: root.open ? Theme.accent : Theme.text
  }
  // The track, then the part that is set.
  Rectangle {
    id: track
    visible: root.reveal > 0.05
    opacity: root.reveal
    anchors.verticalCenter: parent.verticalCenter
    x: 26
    width: 80
    height: 4
    radius: 2
    color: Theme.overlay
    Rectangle {
      width: parent.width * Math.max(0, Math.min(1, root.value))
      height: parent.height
      radius: 2
      color: Theme.accent
    }
    MouseArea {
      id: drag
      anchors { fill: parent; topMargin: -10; bottomMargin: -10 }
      cursorShape: Qt.PointingHandCursor
      function set(m) { root.moved(Math.max(0, Math.min(1, m.x / width))); }
      onPressed: m => set(m)
      onPositionChanged: m => { if (pressed) set(m); }
    }
  }

  // Scrolling over it steps it: 5% per wheel notch (120); a touchpad's small,
  // many deltas add up to the same, rather than 5% each; a sideways scroll
  // changes nothing. On top, taking no buttons, so the track still drags.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.NoButton
    onWheel: e => {
      const d = e.angleDelta.y !== 0 ? e.angleDelta.y / 120 : e.pixelDelta.y / 120;
      if (d !== 0)
        root.moved(Math.max(0, Math.min(1, root.value + 0.05 * d)));
    }
  }
}
