import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The statusline style's bar (Theme.statusline): one slim solid line along
// the bottom, in segments like a terminal's statusline. Left: FRIDAY's state
// (click: open it) and the workspaces; middle: the window; right: volume,
// network and the time (click: the controls; the calendar). No glass, no
// motion: the lightest bar. After Neovim/tmux statuslines and
// Trendorin/yami's (an idea, see CREDITS.md).
PanelWindow {
  id: line

  required property ShellScreen modelData
  screen: modelData

  anchors { bottom: true; left: true; right: true }
  implicitHeight: Theme.barHeight
  exclusiveZone: Theme.barHeight
  color: Theme.surface
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // Dark or light text for a block of colour c: by its relative luminance,
  // the one with more contrast (at least 4.5:1 on every palette's signal
  // and error colours).
  function ink(c) {
    const f = x => x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b) > 0.18 ? "#16181d" : "#f4f5f7";
  }

  // A powerline chevron, drawn (font glyphs don't meet the blocks exactly):
  // a triangle in `from`'s colour pointing right, into what follows, or left.
  component Chevron: Shape {
    id: chev
    property color from
    property bool pointsLeft: false
    Layout.fillHeight: true
    implicitWidth: Math.round(Theme.barHeight / 2)
    implicitHeight: Theme.barHeight
    preferredRendererType: Shape.CurveRenderer
    // (ShapePath and its lines aren't items: they name the shape by id.)
    ShapePath {
      strokeWidth: 0
      strokeColor: "transparent"
      fillColor: chev.from
      startX: chev.pointsLeft ? chev.width : 0; startY: 0
      PathLine { x: chev.pointsLeft ? 0 : chev.width; y: chev.height / 2 }
      PathLine { x: chev.pointsLeft ? chev.width : 0; y: chev.height }
      PathLine { x: chev.pointsLeft ? chev.width : 0; y: 0 }
    }
  }
  // A block of text on a colour; a click does `act`.
  component Segment: Rectangle {
    id: seg
    property string label
    property color fg: Theme.text
    signal act()
    Layout.fillHeight: true
    implicitWidth: segText.implicitWidth + 18
    Text {
      id: segText
      anchors.centerIn: parent
      text: seg.label
      color: seg.fg
      font.family: Theme.mono
      font.pixelSize: Theme.fontS
      font.weight: Font.DemiBold
    }
    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: seg.act() }
  }

  RowLayout {
    anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
    spacing: 0
    Segment {
      color: Friday.mode === "working" ? Theme.signal : Friday.mode === "ready" ? Theme.accent : Theme.error
      // onAccent is matched to the accent only: on the signal or error
      // colour, dark or light text, whichever reads better on it.
      fg: Friday.mode === "ready" ? Theme.onAccent : line.ink(color)
      label: "FRIDAY " + Friday.mode.toUpperCase()
      onAct: Shell.toggle("friday", line.modelData.name)
    }
    Chevron { from: Friday.mode === "working" ? Theme.signal : Friday.mode === "ready" ? Theme.accent : Theme.error }
    Workspaces { Layout.leftMargin: 4; Layout.alignment: Qt.AlignVCenter; screen: line.modelData }
  }

  WindowTitle {
    anchors.centerIn: parent
    width: Math.min(implicitWidth, parent.width * 0.4)
  }

  RowLayout {
    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
    spacing: 0
    Chevron { pointsLeft: true; from: Theme.raised }
    Segment {
      color: Theme.raised
      label: (Audio.muted ? "MUTE" : "VOL " + Math.round(Audio.volume * 100) + "%")
        + (Network.connected ? "  " + (Network.name || Network.kind).toUpperCase() : "  OFFLINE")
      onAct: Shell.toggle("control", line.modelData.name)
    }
    Chevron { pointsLeft: true; from: Theme.accent }
    Segment {
      SystemClock { id: time; precision: SystemClock.Minutes }
      color: Theme.accent
      fg: Theme.onAccent
      label: Qt.formatDateTime(time.date, Config.clock24h ? "ddd d  HH:mm" : "ddd d  h:mm ap")
      onAct: Shell.toggle("calendar", line.modelData.name)
    }
  }
}
