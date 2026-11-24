import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components
import qs.services

ColumnLayout {
  spacing: 16

  RowLayout {
    Layout.fillWidth: true
    spacing: 14
    Orb { size: 44 }
    ColumnLayout {
      spacing: 2
      Label { text: "FRIDAY"; font.family: Theme.display; font.pixelSize: Theme.fontXL; font.letterSpacing: 6 }
      Label {
        text: Friday.headline
        color: Friday.mode === "ready" || Friday.mode === "working" ? Theme.subtext : Theme.warn
      }
    }
  }

  Label {
    Layout.fillWidth: true
    text: "Press Super+Y any time and ask FRIDAY anything. Super+Alt+Y asks about the text you selected, Super+Ctrl+Y about your screen."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { primary: true; text: "Ask FRIDAY something"; onClicked: Quickshell.execDetached(["friday", "ask"]) }
    Chip { text: "How is my computer doing?"; onClicked: Quickshell.execDetached(["friday", "ask", "How is my computer doing?"]) }
    Chip { text: "Show the tour"; onClicked: Quickshell.execDetached(["friday", "tour"]) }
    Chip { visible: Friday.mode === "offline" || Friday.mode === "stopped"; text: "Start FRIDAY"; onClicked: Friday.startDaemon() }
  }
}
