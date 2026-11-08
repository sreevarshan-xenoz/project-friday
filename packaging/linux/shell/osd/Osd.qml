import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// A small pill near the bottom when volume or brightness changes, on the
// focused screen. It fades after a moment.
PanelWindow {
  id: osd

  required property ShellScreen modelData
  screen: modelData

  property string kind: "volume"
  property bool shown: false
  visible: shown || pill.opacity > 0.01

  anchors.bottom: true
  // Clear of a bottom dock.
  margins.bottom: 64 + (Theme.barAtBottom ? Theme.barReach : 0)
  implicitWidth: 260
  implicitHeight: 48
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-osd"
  WlrLayershell.layer: WlrLayer.Overlay
  mask: Region {}

  function show(k) {
    if (Shell.focusedScreen() !== modelData.name)
      return;
    osd.kind = k;
    osd.shown = true;
    hide.restart();
  }

  Timer { id: hide; interval: 1400; onTriggered: osd.shown = false }

  // Volume changes are only shown once the shell has settled, not when
  // PipeWire first reports the current level.
  property bool armed: false
  Timer { interval: 2000; running: true; onTriggered: osd.armed = true }
  Connections {
    target: Audio
    function onVolumeChanged() { if (osd.armed) osd.show("volume"); }
    function onMutedChanged() { if (osd.armed) osd.show("volume"); }
  }
  Connections {
    target: Brightness
    function onChangedExternally() { osd.show("brightness"); }
  }

  readonly property real value: kind === "volume" ? (Audio.muted ? 0 : Audio.volume) : Brightness.value

  Rectangle {
    id: pill
    anchors.fill: parent
    radius: height / 2
    color: Theme.bg
    border.width: 1
    border.color: Theme.outline
    opacity: osd.shown ? 1 : 0
    scale: osd.shown ? 1 : 0.92
    Behavior on opacity { NumberAnimation { duration: Theme.fast } }
    Behavior on scale { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

    RowLayout {
      anchors.fill: parent
      anchors.leftMargin: 16
      anchors.rightMargin: 18
      spacing: 12
      Glyph {
        text: osd.kind === "volume" ? Audio.icon() : Icons.brightness
        size: 18
        color: osd.kind === "volume" ? Theme.accent : Theme.signal
      }
      Rectangle {
        Layout.fillWidth: true
        implicitHeight: 6
        radius: 3
        color: Theme.overlay
        Rectangle {
          width: parent.width * Math.max(0, Math.min(1, osd.value))
          height: parent.height
          radius: 3
          color: osd.kind === "volume" ? Theme.accent : Theme.signal
          Behavior on width { NumberAnimation { duration: Theme.fast } }
        }
      }
      Label {
        Layout.preferredWidth: 34
        horizontalAlignment: Text.AlignRight
        text: Math.round(osd.value * 100)
        font.family: Theme.mono
        font.pixelSize: Theme.fontS + 1
        color: Theme.subtext
      }
    }
  }
}
