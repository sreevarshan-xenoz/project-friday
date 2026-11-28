import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.services

// The desktop pet (shell.json "pet": "mote" and the others; "off" hides it;
// switch: desktop-pet). A small creature in the bottom-right corner that
// shows what FRIDAY is doing: it thinks while FRIDAY works and cheers when
// an answer is ready. A click opens the ask box; a file dropped on it is
// asked about. FRIDAY's own characters, animated once with Google Flow.
//
// Light by design: the window is just the pet's square; between moves it
// shows one still frame (no redraws); a move is a six-second sprite sheet
// at 8 frames a second (a breath about once a minute at rest), and only
// the sheet in use is loaded. Hyprland draws a
// fullscreen window over it.
PanelWindow {
  id: pet

  required property ShellScreen modelData
  screen: modelData

  // Each state is a sheet of 48 square 160 px frames, 8 to a row, in
  // pets/<name>/<state>.png: idle (breathe and blink), think, happy.
  readonly property int frames: 48
  readonly property int cell: 160
  readonly property int size: 112
  readonly property string dir: Qt.resolvedUrl("../pets/" + Config.pet + "/")

  // What it plays: "think" while FRIDAY works, "happy" once when an answer
  // comes, "idle" now and then, else nothing (a still frame).
  property string move: Friday.working ? "think" : ""
  property bool cheered: true

  anchors { bottom: true; right: true }
  margins.right: Theme.gap * 2
  margins.bottom: (Config.dock ? 24 : 0) + (Theme.barAtBottom ? Theme.barReach + Theme.gap : Theme.gap * 2)
  implicitWidth: size
  implicitHeight: size
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-pet"
  WlrLayershell.layer: WlrLayer.Top

  Connections {
    target: Friday
    function onWorkingChanged() {
      if (Friday.working) { pet.cheered = false; pet.move = "think"; }
      else if (!pet.cheered && !Friday.lastFailed) { pet.cheered = true; pet.move = "happy"; }
      else pet.move = "";
    }
  }

  // Now and then, when nothing else is on, it breathes and blinks.
  Timer {
    interval: 45000
    repeat: true
    running: pet.move === "" && Theme.motion
    onTriggered: pet.move = "idle"
  }

  // The frame shown, stepped 8 times a second only while a move plays; a
  // move plays once (think loops while FRIDAY works).
  property int frame: 0
  onMoveChanged: frame = 0
  Timer {
    interval: 125
    repeat: true
    running: pet.move !== "" && Theme.motion
    onTriggered: {
      if (pet.frame < pet.frames - 1) pet.frame++;
      else if (pet.move === "think") pet.frame = 0;
      else pet.move = "";
    }
  }

  // The whole sheet, moved under a clip so one frame shows: a step only
  // moves it, so nothing is decoded or redrawn between steps.
  Item {
    anchors.fill: parent
    clip: true
    opacity: Friday.mode === "offline" ? 0.55 : 1
    scale: tap.pressed ? 0.94 : drop.containsDrag ? 1.08 : 1
    Behavior on scale { NumberAnimation { duration: Theme.fast } }
    Image {
      source: Config.pet === "off" ? "" : pet.dir + (pet.move || "idle") + ".png"
      width: pet.size * 8
      height: pet.size * pet.frames / 8
      sourceSize.width: pet.cell * 8
      x: -(pet.frame % 8) * pet.size
      y: -Math.floor(pet.frame / 8) * pet.size
      smooth: true
      mipmap: true
    }
  }

  MouseArea {
    id: tap
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    onClicked: Quickshell.execDetached(["friday", "ask"])
  }

  // A file dropped on it: ask about that file.
  DropArea {
    id: drop
    anchors.fill: parent
    keys: ["text/uri-list"]
    onDropped: d => {
      const u = d.urls.length ? String(d.urls[0]) : "";
      if (!u.startsWith("file://")) return;
      const path = decodeURIComponent(u.slice(7));
      Quickshell.execDetached(["friday", "ask", "@file \"" + path + "\" What is this, in short?"]);
      d.acceptProposedAction();
    }
  }
}
