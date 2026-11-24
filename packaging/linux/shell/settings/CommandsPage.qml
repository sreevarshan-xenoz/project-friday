import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

// Every friday command with one line on what it does (friday help).
ColumnLayout {
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "Everything FRIDAY can do from a terminal. Most of it is also on the other pages here."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Cmd { id: help; args: ["help"] }
  Component.onCompleted: help.run()

  Output { maxHeight: 520; text: help.output }
}
