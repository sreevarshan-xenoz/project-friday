import QtQuick
import QtQuick.Layouts
import Quickshell.Services.Mpris
import qs.common
import qs.components

// What is playing, if anything. Click to pause or resume.
Item {
  id: root

  readonly property var player: {
    const all = Mpris.players.values;
    return all.find(p => p.isPlaying) || null;
  }

  visible: player !== null
  implicitWidth: row.implicitWidth
  implicitHeight: 24

  RowLayout {
    id: row
    anchors.verticalCenter: parent.verticalCenter
    spacing: 6
    Glyph {
      text: Icons.pause
      size: 14
      color: Theme.signal
    }
    Label {
      Layout.maximumWidth: 220
      text: root.player ? [root.player.trackTitle, root.player.trackArtist].filter(s => s).join(" · ") : ""
      font.pixelSize: Theme.fontS + 1
      color: Theme.subtext
    }
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    onClicked: if (root.player) root.player.togglePlaying()
  }
}
