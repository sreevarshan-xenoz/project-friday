import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  id: page
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
  Cmd { id: forget; onFinished: confirming = "" }

  // What the person is being asked to confirm ("" = nothing).
  property string confirming: ""
  readonly property var forgettable: [
    ["Forget my answers", "answers", "all saved answers"],
    ["Forget what left this computer", "sent", "the list of what was sent to AI providers"],
    ["Forget everything", "all", "answers, that list, memory and the desktop action log"]
  ]

  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { text: sent.running ? "Looking…" : "What left this computer"; onClicked: sent.run() }
    Repeater {
      model: page.forgettable
      Chip {
        required property var modelData
        text: modelData[0]
        onClicked: page.confirming = modelData[1]
      }
    }
  }

  Rectangle {
    visible: confirming !== ""
    Layout.fillWidth: true
    implicitHeight: ask.implicitHeight + 24
    radius: Theme.radiusS
    color: Theme.surface
    border.width: 1
    border.color: Theme.warn
    ColumnLayout {
      id: ask
      anchors.fill: parent
      anchors.margins: 12
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: {
          for (const f of forgettable) if (f[1] === confirming) return "Really forget " + f[2] + "? This can't be undone.";
          return "";
        }
        wrapMode: Text.Wrap
        elide: Text.ElideNone
      }
      Row {
        spacing: 8
        Chip { primary: true; text: forget.running ? "Forgetting…" : "Yes, forget"; onClicked: { forget.args = ["forget", confirming, "--yes"]; forget.run(); } }
        Chip { text: "Cancel"; onClicked: confirming = "" }
      }
    }
  }
  Output { text: forget.output !== "" ? forget.output : sent.output }
}
