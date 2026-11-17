import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  id: page
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "FRIDAY can read its answers out loud, in a voice made on this computer. Nothing is sent anywhere to speak. It downloads once (about 100 MB) and needs no graphics card."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Toggle {
    id: speak
    title: "Read answers out loud"
    detail: "FRIDAY reads each answer as it appears, a sentence at a time. Code is skipped."
    statusArgs: ["voice", "speak", "status"]
    onArgs: ["voice", "speak", "on"]
    offArgs: ["voice", "speak", "off"]
    onText: "Spoken answers: on"
  }

  Cmd {
    id: setup
    args: ["voice", "speak", "setup"]
    onFinished: speak.refresh()
  }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip {
      primary: true
      text: setup.running ? "Downloading…" : "Download the voice"
      onClicked: setup.run()
    }
    Chip { text: "Hear it"; onClicked: Quickshell.execDetached(["friday", "say", "Hi, I'm FRIDAY. This is how I sound."]) }
    Chip { text: "Stop"; onClicked: Quickshell.execDetached(["friday", "say", "--stop"]) }
  }
  Output { text: setup.output }

  Label { text: "Voice"; font.weight: Font.DemiBold }
  Flow {
    Layout.fillWidth: true
    spacing: 6
    Repeater {
      model: 10
      Chip {
        required property int index
        text: String(index + 1)
        onClicked: Quickshell.execDetached(["friday", "voice", "speak", "voice", String(index)])
      }
    }
  }
}
