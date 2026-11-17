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
    text: "FRIDAY can read its answers out loud, in a voice made on this computer. Nothing is sent anywhere to speak. It downloads once (about 50 MB) and needs no graphics card. Kokoro is a bigger, more expressive voice (about 150 MB)."
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
    onFinished: { speak.refresh(); voices.run(); }
  }
  Cmd {
    id: small
    args: ["voice", "speak", "setup", "kokoro"]
    onFinished: { speak.refresh(); voices.run(); }
  }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip {
      primary: true
      text: setup.running ? "Downloading…" : "Download the voice"
      onClicked: setup.run()
    }
    Chip {
      text: small.running ? "Downloading…" : "Kokoro, more expressive (150 MB)"
      onClicked: small.run()
    }
    Chip { text: "Hear it"; onClicked: Quickshell.execDetached(["friday", "say", "Hi, I'm FRIDAY. This is how I sound."]) }
    Chip { text: "Stop"; onClicked: Quickshell.execDetached(["friday", "say", "--stop"]) }
  }
  Output { text: setup.output + small.output }

  // The installed voice's names, as `friday voice speak voices` prints them ("0 Bella").
  property var names: []
  Cmd {
    id: voices
    args: ["voice", "speak", "voices"]
    onFinished: output => page.names = output.split("\n")
      .map(l => l.trim().match(/^(\d+) (.+)$/)).filter(m => m !== null).map(m => ({ n: Number(m[1]), name: m[2] }))
  }
  Component.onCompleted: voices.run()

  Label { visible: page.names.length > 0; text: "Voice"; font.weight: Font.DemiBold }
  Flow {
    Layout.fillWidth: true
    spacing: 6
    Repeater {
      model: page.names
      Chip {
        required property var modelData
        text: modelData.name
        onClicked: {
          Quickshell.execDetached(["friday", "voice", "speak", "voice", String(modelData.n)]);
          Quickshell.execDetached(["friday", "say", "Hi, I'm FRIDAY. This is how I sound."]);
        }
      }
    }
  }
}
