import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Widgets
import qs.common
import qs.components
import qs.services

// The aperture island: the bar's centre, one capsule that becomes what
// FRIDAY and the computer are doing, morphing from one face to the next.
//   idle       the orb and the clock (orb: FRIDAY; clock: calendar)
//   listening  a voice wave while FRIDAY hears you
//   working    the thinking aura, the phase and the question
//   done       a check for a moment when an answer is ready
//   media      on hover while something plays: art, title, controls
// Nothing moves while it rests: animations run on a change, on hover, and
// while listening or working (when the orb moves anyway).
Item {
  id: island

  required property string screenName
  // The bar flips the island to grow upward when it sits at the bottom.
  property bool growUp: false

  readonly property bool listening: Answer.state === "listening"
  readonly property bool working: Friday.mode === "working"
  property bool done: false
  property bool expanded: false
  readonly property string face: listening ? "listening"
    : working ? "working"
    : done ? "done"
    : expanded && Media.player && !Answer.visible ? "media"
    : "idle"

  // An answer that just finished: a check for 2.5 s.
  property string lastMode: Friday.mode
  Connections {
    target: Friday
    function onModeChanged() {
      if (island.lastMode === "working" && Friday.mode === "ready") {
        island.done = true;
        doneTimer.restart();
      }
      island.lastMode = Friday.mode;
    }
  }
  Timer { id: doneTimer; interval: 2500; onTriggered: island.done = false }

  // Resting on the playing bars opens the player (the clock stays clickable);
  // leaving the island closes it a moment later.
  Timer { id: openTimer; interval: 300; onTriggered: island.expanded = eqHover.hovered }
  Timer { id: closeTimer; interval: 400; onTriggered: if (!hover.hovered) island.expanded = false }

  readonly property real restH: Theme.barHeight - 8
  readonly property real openH: 64
  readonly property Item faceItem: face === "listening" ? listenFace
    : face === "working" ? workFace
    : face === "done" ? doneFace
    : face === "media" ? mediaFace
    : idleFace

  implicitWidth: cap.width
  implicitHeight: restH
  // The capsule itself, for the bar's input mask (it outgrows this item open).
  readonly property Item capsule: cap

  Rectangle {
    id: cap
    width: Math.round(island.faceItem.implicitWidth + 2 * 14)
    height: island.face === "media" ? island.openH : island.restH
    y: island.growUp ? island.restH - height : 0
    radius: Theme.sharp ? 3 : Math.min(height / 2, 22)
    color: island.face === "idle" ? (hover.hovered ? Theme.surface : "transparent")
      : island.face === "working" || island.face === "listening" ? Theme.alpha(Theme.accent, 0.14)
      : Theme.raised
    border.width: island.face === "idle" ? 0 : 1
    border.color: island.face === "working" || island.face === "listening" ? Theme.alpha(Theme.accent, 0.5) : Theme.glassEdge
    clip: true
    // A little lift under the pointer.
    scale: hover.hovered && island.face === "idle" ? 1.04 : 1

    Behavior on width { NumberAnimation { duration: Theme.medium; easing.type: Theme.spring; easing.overshoot: 1.05 } }
    Behavior on height { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    Behavior on y { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    Behavior on color { ColorAnimation { duration: Theme.medium } }
    Behavior on scale { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }

    HoverHandler {
      id: hover
      onHoveredChanged: {
        if (hovered) closeTimer.stop();
        else { openTimer.stop(); closeTimer.restart(); }
      }
    }
    // Scrolling over the player changes the volume. (A button-less
    // MouseArea: a WheelHandler here never got the wheel events.)
    MouseArea {
      anchors.fill: parent
      enabled: island.face === "media"
      acceptedButtons: Qt.NoButton
      onWheel: e => {
        if (e.angleDelta.y !== 0)
          Audio.setVolume(Math.max(0, Math.min(1, Audio.volume + (e.angleDelta.y > 0 ? 0.05 : -0.05))));
      }
    }

    // A ripple from where it was clicked.
    Rectangle {
      id: ripple
      width: 0; height: width; radius: width / 2
      color: Theme.alpha(Theme.accent, 0.35)
      opacity: 0
      ParallelAnimation {
        id: rippleAnim
        NumberAnimation { target: ripple; property: "width"; from: 0; to: cap.width * 1.4; duration: Theme.slow; easing.type: Theme.easing }
        NumberAnimation { target: ripple; property: "opacity"; from: 1; to: 0; duration: Theme.slow; easing.type: Theme.easing }
      }
      function burst(px, py) {
        if (!Theme.motion) return;
        rippleAnim.stop();
        x = Qt.binding(() => px - width / 2);
        y = Qt.binding(() => py - width / 2);
        rippleAnim.start();
      }
    }

    // Each face fades and settles in when it comes, out when it goes.
    component Face: Item {
      required property string name
      readonly property bool on: island.face === name
      anchors.centerIn: parent
      width: implicitWidth
      height: implicitHeight
      opacity: on ? 1 : 0
      scale: on ? 1 : 0.9
      visible: opacity > 0.01
      Behavior on opacity { NumberAnimation { duration: Theme.fast + 40 } }
      Behavior on scale { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    }

    Face {
      id: idleFace
      name: "idle"
      implicitWidth: idleRow.implicitWidth
      implicitHeight: idleRow.implicitHeight
      RowLayout {
        id: idleRow
        spacing: 10
        Item {
          implicitWidth: 18; implicitHeight: 18
          Orb { anchors.centerIn: parent; size: 18 }
          MouseArea {
            anchors.fill: parent; anchors.margins: -6
            cursorShape: Qt.PointingHandCursor
            onClicked: m => { ripple.burst(mapToItem(cap, m.x, m.y).x, mapToItem(cap, m.x, m.y).y); Shell.toggle("friday", island.screenName); }
          }
        }
        RollingText {
          text: Qt.formatDateTime(time.date, Config.clock24h ? "ddd d MMM   HH:mm" : "ddd d MMM   h:mm AP")
          family: Theme.mono
          pixelSize: Theme.fontM
          weight: Font.Medium
          color: clockMouse.containsMouse || Shell.isOpen("calendar", island.screenName) ? Theme.accent : Theme.text
          SystemClock { id: time; precision: SystemClock.Minutes }
          MouseArea {
            id: clockMouse
            anchors.fill: parent; anchors.margins: -6
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: m => { ripple.burst(mapToItem(cap, m.x, m.y).x, mapToItem(cap, m.x, m.y).y); Shell.toggle("calendar", island.screenName); }
          }
        }
        Label {
          visible: Friday.mode === "offline" || Friday.mode === "stopped"
          text: Friday.mode === "stopped" ? "FRIDAY " + Friday.status : "FRIDAY offline"
          color: Theme.subtext
          font.pixelSize: Theme.fontS + 1
        }
        // A player: three bars that step four times a second while it plays
        // (still and dim while paused); rest on them to open the player.
        Row {
          visible: Media.player !== null
          opacity: Media.playing ? 1 : 0.45
          spacing: 2
          Layout.alignment: Qt.AlignVCenter
          Repeater {
            model: 3
            Rectangle {
              required property int index
              width: 3; radius: 1.5
              height: 4 + ((eq.step + index * 2) % 4) * 3
              anchors.bottom: parent.bottom
              color: Theme.signal
            }
          }
          height: 14
          HoverHandler {
            id: eqHover
            margin: 8
            onHoveredChanged: if (hovered && Media.player) openTimer.restart()
          }
          // They dance for 8 s when a track starts or changes, and while the
          // pointer is on them, then hold still: no redraws while music plays.
          Timer { id: eq; property int step: 0; interval: 250; repeat: true; running: Media.playing && Theme.motion && (eqLive.running || eqHover.hovered); onTriggered: step++ }
          Timer { id: eqLive; interval: 8000 }
          Connections {
            target: Media
            function onPlayingChanged() { if (Media.playing) eqLive.restart(); }
            function onPlayerChanged() { eqLive.restart(); }
          }
          Connections {
            target: Media.player
            ignoreUnknownSignals: true
            function onTrackTitleChanged() { eqLive.restart(); }
          }
        }
      }
    }

    Face {
      id: listenFace
      name: "listening"
      implicitWidth: listenRow.implicitWidth
      implicitHeight: listenRow.implicitHeight
      RowLayout {
        id: listenRow
        spacing: 10
        // A voice wave: seven bars breathing out of step.
        Row {
          spacing: 3
          height: 18
          Repeater {
            model: 7
            Rectangle {
              required property int index
              width: 3; radius: 1.5
              anchors.verticalCenter: parent.verticalCenter
              color: Theme.accent
              property real k: 0.3
              height: 4 + 14 * k
              SequentialAnimation on k {
                running: listenFace.on && Theme.motion
                loops: Animation.Infinite
                PauseAnimation { duration: index * 70 }
                NumberAnimation { to: 1; duration: 260 + index * 30; easing.type: Easing.InOutSine }
                NumberAnimation { to: 0.2; duration: 300 + index * 20; easing.type: Easing.InOutSine }
              }
            }
          }
        }
        Label {
          text: Answer.listenHint ? "Listening… " + Answer.listenHint : "Listening…"
          color: Theme.text
          font.weight: Font.Medium
        }
      }
    }

    Face {
      id: workFace
      name: "working"
      implicitWidth: workRow.implicitWidth
      implicitHeight: workRow.implicitHeight
      RowLayout {
        id: workRow
        spacing: 9
        Orb { size: 18; mode: "working" }
        Label {
          Layout.maximumWidth: 340
          text: (Friday.phase || "working") + (Friday.goal ? " · " + Friday.goal : "")
          color: Theme.accent
          font.pixelSize: Theme.fontS + 1
          font.weight: Font.Medium
        }
      }
      // A thin light running along the bottom edge while FRIDAY works.
      Rectangle {
        parent: cap
        visible: workFace.on && Theme.motion
        height: 2; radius: 1
        y: cap.height - 3
        width: cap.width * 0.25
        color: Theme.accent
        opacity: 0.8
        NumberAnimation on x {
          running: workFace.on && Theme.motion
          from: -cap.width * 0.25; to: cap.width
          duration: 1400; loops: Animation.Infinite; easing.type: Easing.InOutQuad
        }
      }
    }

    Face {
      id: doneFace
      name: "done"
      implicitWidth: doneRow.implicitWidth
      implicitHeight: doneRow.implicitHeight
      RowLayout {
        id: doneRow
        spacing: 8
        Glyph {
          text: Icons.check
          size: 16
          color: Theme.ok
          scale: doneFace.on ? 1 : 0.2
          Behavior on scale { NumberAnimation { duration: Theme.slow; easing.type: Theme.spring; easing.overshoot: 2 } }
        }
        Label { text: Answer.ok === false ? "Answered, with a problem" : "Answer ready"; font.weight: Font.Medium }
      }
    }

    Face {
      id: mediaFace
      name: "media"
      readonly property var player: Media.player
      implicitWidth: mediaRow.implicitWidth
      implicitHeight: mediaRow.implicitHeight
      RowLayout {
        id: mediaRow
        spacing: 12
        ClippingRectangle {
          implicitWidth: 44; implicitHeight: 44
          radius: Theme.sharp ? 2 : 12
          color: Theme.overlay
          Image {
            anchors.fill: parent
            source: mediaFace.player && mediaFace.on ? mediaFace.player.trackArtUrl : ""
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            sourceSize: Qt.size(88, 88)
          }
        }
        ColumnLayout {
          spacing: 1
          Label {
            Layout.maximumWidth: 220
            text: mediaFace.player ? mediaFace.player.trackTitle || "Unknown" : ""
            font.weight: Font.DemiBold
          }
          Label {
            Layout.maximumWidth: 220
            text: mediaFace.player ? mediaFace.player.trackArtist || "" : ""
            color: Theme.subtext
            font.pixelSize: Theme.fontS
          }
        }
        IconButton { tooltip: "Previous"; icon: Icons.previous; size: 28; onClicked: if (mediaFace.player) mediaFace.player.previous() }
        IconButton {
          tooltip: Media.playing ? "Pause" : "Play"
          icon: Media.playing ? Icons.pause : Icons.play
          size: 32
          active: true
          onClicked: if (mediaFace.player) mediaFace.player.togglePlaying()
        }
        IconButton { tooltip: "Next"; icon: Icons.next; size: 28; onClicked: if (mediaFace.player) mediaFace.player.next() }
      }
    }
  }
}
