import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Widgets
import qs.common
import qs.components
import qs.services

// Widgets on the desktop, under every window: the time, FRIDAY (ask from
// here; the last answer; standing tasks), the computer (CPU and memory) and
// what's playing. Each leans toward the pointer and can be dragged anywhere.
// friday feature desktop-widgets off hides them.
PanelWindow {
  id: desk

  required property ShellScreen modelData
  screen: modelData
  visible: Config.featureOn("desktop-widgets")

  anchors { top: true; bottom: true; left: true; right: true }
  margins.top: Theme.barAtBottom ? 0 : Theme.barReach
  margins.bottom: Theme.barAtBottom ? Theme.barReach : 0
  margins.left: Theme.leftReach
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-desktop"
  WlrLayershell.layer: WlrLayer.Bottom
  // The ask field takes the keyboard only when clicked.
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

  // Only the cards take the pointer; the rest of the desktop is untouched.
  mask: Region {
    Region { item: clockCard }
    Region { item: fridayCard }
    Region { item: systemCard }
    Region { item: mediaCard.visible ? mediaCard : null }
  }

  Item {
    id: area
    anchors.fill: parent
    anchors.margins: 32

    // ---- The time: hours and minutes staggered, like the lock screen.
    DeskCard {
      id: clockCard
      key: "clock"
      homeX: 0.0; homeY: 0.92
      color: "transparent"
      border.width: 0
      implicitWidth: clockCol.implicitWidth + 40
      width: implicitWidth
      height: clockCol.implicitHeight + 32
      SystemClock { id: now; precision: SystemClock.Minutes }
      ColumnLayout {
        id: clockCol
        x: 20; y: 16
        spacing: 2
        Item {
          implicitWidth: hours.implicitWidth * 0.9 + minutes.implicitWidth
          implicitHeight: hours.implicitHeight + 34
          RollingText {
            id: hours
            text: Qt.formatTime(now.date, Config.clock24h ? "HH" : "hh")
            family: Theme.display
            pixelSize: 84
            weight: Font.Normal
            color: Theme.text
          }
          RollingText {
            id: minutes
            x: hours.implicitWidth * 0.9
            y: 34
            text: Qt.formatTime(now.date, "mm")
            family: Theme.display
            pixelSize: 84
            weight: Font.Normal
            color: Theme.accent
          }
        }
        Label {
          text: Qt.formatDate(now.date, "dddd, d MMMM")
          color: Theme.subtext
          font.pixelSize: Theme.fontL
        }
      }
    }

    // ---- FRIDAY: ask from the desktop, the last answer, standing tasks.
    DeskCard {
      id: fridayCard
      key: "friday"
      homeX: 1.0; homeY: 0.08
      width: 340
      height: fridayCol.implicitHeight + 36
      ColumnLayout {
        id: fridayCol
        x: 18; y: 18
        width: parent.width - 36
        spacing: 12
        RowLayout {
          spacing: 12
          // The orb drifts a little toward the pointer, as if it's looking.
          Item {
            implicitWidth: 44; implicitHeight: 44
            Orb {
              size: 40
              anchors.centerIn: parent
              anchors.horizontalCenterOffset: (fridayCard.px - 0.5) * 10
              anchors.verticalCenterOffset: (fridayCard.py - 0.5) * 10
              Behavior on anchors.horizontalCenterOffset { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
              Behavior on anchors.verticalCenterOffset { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
            }
          }
          ColumnLayout {
            spacing: 0
            Label { text: "FRIDAY"; font.family: Theme.display; font.pixelSize: Theme.fontL; font.letterSpacing: 4 }
            Label { text: Friday.headline; color: Theme.subtext; font.pixelSize: Theme.fontS }
          }
        }
        // Ask without opening anything: Enter sends it.
        Rectangle {
          Layout.fillWidth: true
          implicitHeight: 40
          radius: 20
          color: Theme.glassWell
          border.width: 1
          border.color: ask.activeFocus ? Theme.accent : Theme.glassEdge
          TextInput {
            id: ask
            anchors.fill: parent
            anchors.leftMargin: 16; anchors.rightMargin: 16
            verticalAlignment: TextInput.AlignVCenter
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: Theme.fontM + 1
            clip: true
            onAccepted: {
              const q = text.trim();
              if (q) Quickshell.execDetached(["friday", "ask", q]);
              text = "";
              focus = false;
            }
            Keys.onEscapePressed: { text = ""; focus = false; }
          }
          Label {
            anchors.fill: parent
            anchors.leftMargin: 16
            visible: ask.text === ""
            text: "Ask FRIDAY anything…"
            color: Theme.faint
          }
        }
        // The last answer, a line of it; click for the FRIDAY drawer.
        ColumnLayout {
          Layout.fillWidth: true
          visible: History.entries.length > 0
          spacing: 2
          Label { text: "LAST ANSWER"; color: Theme.faint; font.pixelSize: Theme.fontS; font.letterSpacing: 1.5 }
          Label {
            Layout.fillWidth: true
            text: History.entries.length ? History.entries[0].goal : ""
            font.weight: Font.DemiBold
          }
          Label {
            Layout.fillWidth: true
            text: History.entries.length ? History.entries[0].body.replace(/\s+/g, " ") : ""
            color: Theme.subtext
            font.pixelSize: Theme.fontS + 1
            maximumLineCount: 2
            wrapMode: Text.Wrap
            elide: Text.ElideRight
          }
          TapHandler { onTapped: Shell.toggle("friday", desk.modelData.name) }
        }
        Label {
          visible: Tasks.entries.length > 0
          text: Tasks.entries.length === 1 ? "1 standing task" : Tasks.entries.length + " standing tasks"
          color: Theme.signal
          font.pixelSize: Theme.fontS + 1
        }
      }
    }

    // ---- The computer: CPU and memory, read every 5 s from /proc.
    DeskCard {
      id: systemCard
      key: "system"
      homeX: 1.0; homeY: 0.62
      width: sysRow.implicitWidth + 40
      height: sysRow.implicitHeight + 36

      property real cpu: 0
      property real mem: 0
      property var lastCpu: null
      FileView { id: stat; path: "/proc/stat" }
      FileView { id: meminfo; path: "/proc/meminfo" }
      Timer {
        interval: 5000; running: desk.visible; repeat: true; triggeredOnStart: true
        onTriggered: {
          stat.reload(); meminfo.reload();
          const f = (stat.text().split("\n")[0] || "").trim().split(/\s+/).slice(1).map(Number);
          const idle = (f[3] || 0) + (f[4] || 0), total = f.reduce((a, b) => a + b, 0);
          if (systemCard.lastCpu && total > systemCard.lastCpu.total)
            systemCard.cpu = 1 - (idle - systemCard.lastCpu.idle) / (total - systemCard.lastCpu.total);
          systemCard.lastCpu = { idle: idle, total: total };
          const m = meminfo.text();
          const kb = k => Number((m.match(new RegExp("^" + k + ":\\s+(\\d+)", "m")) || [0, 0])[1]);
          if (kb("MemTotal") > 0) systemCard.mem = 1 - kb("MemAvailable") / kb("MemTotal");
        }
      }

      component Ring: Item {
        property real value: 0
        property string label: ""
        property color tint: Theme.accent
        implicitWidth: 92; implicitHeight: 92
        // Set at once, not eased: each eased step redraws the whole desktop
        // surface (about 40 frames every 5 s), for a change you barely see.
        readonly property real shown: value
        Shape {
          anchors.fill: parent
          preferredRendererType: Shape.CurveRenderer
          ShapePath {
            strokeColor: Theme.overlay; strokeWidth: 7; fillColor: "transparent"; capStyle: ShapePath.RoundCap
            PathAngleArc { centerX: 46; centerY: 46; radiusX: 38; radiusY: 38; startAngle: 0; sweepAngle: 360 }
          }
          ShapePath {
            strokeColor: tint; strokeWidth: 7; fillColor: "transparent"; capStyle: ShapePath.RoundCap
            PathAngleArc { centerX: 46; centerY: 46; radiusX: 38; radiusY: 38; startAngle: -90; sweepAngle: 360 * Math.max(0.01, shown) }
          }
        }
        Column {
          anchors.centerIn: parent
          Label { anchors.horizontalCenter: parent.horizontalCenter; text: Math.round(100 * value) + "%"; font.family: Theme.mono; font.pixelSize: Theme.fontL; font.weight: Font.DemiBold }
          Label { anchors.horizontalCenter: parent.horizontalCenter; text: label; color: Theme.faint; font.pixelSize: Theme.fontS }
        }
      }

      RowLayout {
        id: sysRow
        x: 20; y: 18
        spacing: 18
        Ring { value: systemCard.cpu; label: "CPU"; tint: Theme.accent }
        Ring { value: systemCard.mem; label: "Memory"; tint: Theme.signal }
      }
    }

    // ---- What's playing: only while there's a player.
    DeskCard {
      id: mediaCard
      key: "media"
      homeX: 0.0; homeY: 0.08
      visible: Media.player !== null
      width: 320
      height: mediaRow.implicitHeight + 32
      readonly property var player: Media.player
      RowLayout {
        id: mediaRow
        x: 16; y: 16
        width: parent.width - 32
        spacing: 14
        Item {
          implicitWidth: 72; implicitHeight: 72
          Rectangle { anchors.fill: parent; radius: 16; color: Theme.overlay }
          Glyph { anchors.centerIn: parent; text: Icons.play; size: 26; color: Theme.faint; visible: art.status !== Image.Ready }
          ClippingRectangle {
            anchors.fill: parent
            radius: 16
            color: "transparent"
            visible: art.status === Image.Ready
            Image {
              id: art
              anchors.fill: parent
              source: mediaCard.player ? mediaCard.player.trackArtUrl : ""
              fillMode: Image.PreserveAspectCrop
              asynchronous: true
              sourceSize: Qt.size(144, 144)
            }
          }
        }
        ColumnLayout {
          Layout.fillWidth: true
          spacing: 2
          Label { Layout.fillWidth: true; text: mediaCard.player ? mediaCard.player.trackTitle || "Unknown" : ""; font.weight: Font.DemiBold }
          Label { Layout.fillWidth: true; text: mediaCard.player ? mediaCard.player.trackArtist || "" : ""; color: Theme.subtext; font.pixelSize: Theme.fontS + 1 }
          RowLayout {
            spacing: 6
            IconButton { tooltip: "Previous"; icon: Icons.previous; size: 28; onClicked: if (mediaCard.player) mediaCard.player.previous() }
            IconButton {
              tooltip: Media.playing ? "Pause" : "Play"
              icon: Media.playing ? Icons.pause : Icons.play
              size: 32; active: true
              onClicked: if (mediaCard.player) mediaCard.player.togglePlaying()
            }
            IconButton { tooltip: "Next"; icon: Icons.next; size: 28; onClicked: if (mediaCard.player) mediaCard.player.next() }
          }
        }
      }
    }
  }
}
