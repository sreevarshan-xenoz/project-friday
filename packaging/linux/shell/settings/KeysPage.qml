import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

// Every key and what it does, read from Hyprland itself (friday keys list),
// so the sheet shows the keys you really have. Super+K opens this page.
ColumnLayout {
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "Every key on this desktop and what it does. Super+K opens this page; Super+I opens Settings."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Cmd { id: keys; args: ["keys", "list"] }
  Component.onCompleted: keys.run()

  Output { maxHeight: 520; text: keys.output }
}
