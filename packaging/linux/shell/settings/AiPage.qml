import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "FRIDAY thinks with a language model. Free models work with no account, but they may keep what you send. For anything private, use your own account or key, or keep everything on this computer."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  function inTerminal(title, cmd) {
    Quickshell.execDetached(["foot", "-T", "FRIDAY · " + title, "bash", "-c",
      cmd + "; echo; read -rsn1 -p 'Press any key to close.'"]);
  }

  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { primary: true; text: "Choose my AI and key"; onClicked: inTerminal("choose your AI", "friday setup-ai") }
    Chip { text: "Use an account I already have"; onClicked: inTerminal("connect an account", "friday connect") }
    Chip { text: "Keep everything on this computer"; onClicked: inTerminal("private mode", "friday private") }
  }
  Label {
    Layout.fillWidth: true
    text: "These open a small window that walks you through it."
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }
}
