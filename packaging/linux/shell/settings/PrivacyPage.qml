import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  spacing: 16

  Toggle {
    title: "Name the app when asking about a selection"
    detail: "A question about selected text says which app and window it came from, so answers fit. Off: only the text is sent."
    statusArgs: ["app-context", "status"]
    onArgs: ["app-context", "on"]
    offArgs: ["app-context", "off"]
    onText: "On:"
  }
  Toggle {
    title: "Read a window's text for screen questions"
    detail: "FRIDAY may send a window's text instead of a screenshot when you ask about your screen. The text can include words you can't see. Password fields are skipped. Off by default."
    statusArgs: ["screen-text", "status"]
    onArgs: ["screen-text", "on"]
    offArgs: ["screen-text", "off"]
    onText: "Screen text: on"
  }

  Cmd { id: sent; args: ["sent", "20"] }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { text: sent.running ? "Looking…" : "What left this computer"; onClicked: sent.run() }
    Chip {
      text: "Forget my saved data"
      onClicked: Quickshell.execDetached(["foot", "-T", "FRIDAY · forget", "bash", "-c",
        "friday forget; echo; read -rsn1 -p 'Press any key to close.'"])
    }
  }
  Output { text: sent.output }
}
