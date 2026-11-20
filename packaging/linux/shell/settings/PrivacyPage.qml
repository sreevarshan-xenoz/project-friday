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
    // The detail above is the disclosure the CLI would otherwise ask about.
    onArgs: ["screen-text", "on", "--yes"]
    offArgs: ["screen-text", "off"]
    onText: "Screen text: on"
  }
  Toggle {
    title: "Remember what was on screen (\"what was that…?\")"
    detail: "Keeps the text of the window in front every 30 seconds, on this computer only, for 3 hours. No screenshots; never password fields or private windows (a Chromium incognito window can't be told apart: don't use recall with one open). When you ask \"what was that…\", the matching passages go to your local model, or to your cloud provider if you have none. Turning it off forgets it all."
    statusArgs: ["recall", "status"]
    onArgs: ["recall", "on", "--yes"]
    offArgs: ["recall", "off"]
    onText: "Recall: on"
  }
  Toggle {
    title: "Listen for \"hey Friday\""
    detail: "Listens to your microphone all the time, on this computer only, and turns speech into text with the local voice model. Nothing is recorded or sent unless a piece begins with \"hey Friday\"; then only what you say next is asked. A microphone mark shows in the bar while it listens. Needs voice set up (friday voice setup). Off by default."
    statusArgs: ["wake", "status"]
    onArgs: ["wake", "on", "--yes"]
    offArgs: ["wake", "off"]
    onText: "Wake word: on"
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
