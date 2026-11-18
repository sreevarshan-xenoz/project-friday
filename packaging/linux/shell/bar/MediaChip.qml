import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components
import qs.services

// What is playing, or paused with a track loaded, if anything. Click to pause
// or resume: a paused one stays (dimmed) so it can be resumed from the bar.
Item {
  id: root

  readonly property var player: Media.player
  readonly property bool playing: Media.playing

  visible: player !== null
  implicitWidth: row.implicitWidth
  implicitHeight: 24

  RowLayout {
    id: row
    anchors.verticalCenter: parent.verticalCenter
    spacing: 6
    Glyph {
      text: root.playing ? Icons.pause : Icons.play
      size: 14
      color: root.playing ? Theme.signal : Theme.faint
    }
    Label {
      Layout.maximumWidth: 220
      text: root.player ? [root.player.trackTitle, root.player.trackArtist].filter(s => s).join(" · ") : ""
      font.pixelSize: Theme.fontS + 1
      color: root.playing ? Theme.subtext : Theme.faint
    }
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    onClicked: if (root.player) root.player.togglePlaying()
  }
}
