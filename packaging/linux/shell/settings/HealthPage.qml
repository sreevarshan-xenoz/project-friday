import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "Something not working? Let FRIDAY look. It only looks; it never fixes anything unless you ask."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Cmd { id: check; args: ["doctor"] }
  Cmd { id: report; args: ["report"] }
  Cmd { id: restart; args: ["daemon", "restart"] }

  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { primary: true; text: "How is my computer doing?"; onClicked: Quickshell.execDetached(["friday", "ask", "How is my computer doing?"]) }
    Chip { text: check.running ? "Checking…" : "Check FRIDAY itself"; onClicked: check.run() }
    Chip { text: restart.running ? "Restarting…" : "Restart FRIDAY"; onClicked: restart.run() }
    Chip { text: report.running ? "Saving…" : "Save a report to share"; onClicked: report.run() }
  }
  Output { text: check.output !== "" ? check.output : report.output !== "" ? report.output : restart.output }
}
