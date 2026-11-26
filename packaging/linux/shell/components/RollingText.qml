import QtQuick
import qs.common

// Text whose changed characters roll like an odometer: the old one slides up
// and out, the new one comes up from below. Only characters that change
// move, and only when they change (the clock: once a minute), so it costs
// nothing in between. With motion off (friday feature motion off) they
// change in place. Meant for a monospaced font, so the width never jumps.
Item {
  id: root
  implicitWidth: line.implicitWidth
  implicitHeight: line.implicitHeight

  property string text: ""
  property string family: Theme.mono
  property int pixelSize: Theme.fontM
  property int weight: Font.Medium
  property color color: Theme.text

  component Face: Text {
    color: root.color
    font.family: root.family
    font.pixelSize: root.pixelSize
    font.weight: root.weight
    renderType: Text.NativeRendering
  }

  Row {
    id: line
    Repeater {
      model: root.text.length

      Item {
        id: slot
        required property int index
        readonly property string ch: root.text.charAt(index)
        property string shown: ch
        property string gone: ""

        implicitWidth: now.implicitWidth
        implicitHeight: now.implicitHeight
        clip: true

        onChChanged: {
          if (ch === shown) return;
          gone = shown;
          shown = ch;
          if (Theme.motion) roll.restart();
        }

        Face { id: old; text: slot.gone; y: -slot.height; opacity: 0 }
        Face { id: now; text: slot.shown }

        ParallelAnimation {
          id: roll
          NumberAnimation { target: now; property: "y"; from: slot.height * 0.9; to: 0; duration: Theme.slow; easing.type: Theme.spring }
          NumberAnimation { target: now; property: "opacity"; from: 0; to: 1; duration: Theme.medium }
          NumberAnimation { target: old; property: "y"; from: 0; to: -slot.height * 0.9; duration: Theme.medium; easing.type: Theme.easing }
          NumberAnimation { target: old; property: "opacity"; from: 1; to: 0; duration: Theme.medium }
        }
      }
    }
  }
}
