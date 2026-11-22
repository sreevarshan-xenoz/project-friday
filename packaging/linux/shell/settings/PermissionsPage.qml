import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

ColumnLayout {
  id: page
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "What FRIDAY is allowed to do on this computer. Every switch shows the real setting, and anything that deletes, closes or sends still asks first."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  Label { text: "How much FRIDAY may do on its own"; font.weight: Font.DemiBold }
  Cmd { id: autonomyStatus; args: ["autonomy", "status"] }
  Cmd { id: autonomySet; onFinished: autonomyStatus.run() }
  RowLayout {
    spacing: 8
    Repeater {
      model: [
        { mode: "ask-all", label: "Ask before every action" },
        { mode: "ask-risky", label: "Ask only for risky ones" },
        { mode: "act", label: "Act, then tell me" }
      ]
      Chip {
        required property var modelData
        text: modelData.label
        primary: autonomyStatus.output.indexOf("Autonomy: " + modelData.mode + " ") === 0
        onClicked: { autonomySet.args = ["autonomy", modelData.mode]; autonomySet.run(); }
      }
    }
  }
  Label {
    Layout.fillWidth: true
    text: autonomyStatus.output.replace(/^Autonomy: [a-z-]+ \(?/, "").replace(/\)\s*$/, "")
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
    font.pixelSize: Theme.fontS + 1
  }

  Toggle {
    title: "Control my windows and apps"
    detail: "FRIDAY can focus, move and open windows, change the volume and music, and press buttons in apps. Closing a window or pressing something like Delete asks first. Turning it off restarts FRIDAY."
    statusArgs: ["desktop", "status"]
    onArgs: ["desktop", "on"]
    offArgs: ["desktop", "off"]
    onText: "Desktop control: on"
  }
  Toggle {
    title: "Run things on a schedule"
    detail: "\"Every morning at 8 tell me the weather\": FRIDAY repeats what you asked, with the same rules as any ask. Off: nothing repeats and no new ones can be made. Turning it off restarts FRIDAY."
    statusArgs: ["tasks", "status"]
    onArgs: ["tasks", "on"]
    offArgs: ["tasks", "off"]
    onText: "Standing tasks: on"
  }

  Label { text: "Your phone"; font.weight: Font.DemiBold; Layout.topMargin: 8 }
  Cmd { id: phoneStatus; args: ["phone", "status"] }
  Cmd { id: phoneOn; args: ["phone", "on", "--yes"]; onFinished: phoneStatus.run() }
  Cmd { id: phoneOff; args: ["phone", "off"]; onFinished: phoneStatus.run() }
  Component.onCompleted: { phoneStatus.run(); faceStatus.run(); helperStatus.run(); autonomyStatus.run(); }
  Label {
    Layout.fillWidth: true
    text: phoneStatus.output !== "" ? phoneStatus.output : "Checking…"
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { primary: true; text: phoneOn.running ? "Starting…" : "Pair a phone"; onClicked: page.confirmingPhone = true }
    Chip { text: phoneOff.running ? "Stopping…" : "Turn the phone off"; onClicked: phoneOff.run() }
  }
  // The same words `friday phone on` shows before it asks.
  property bool confirmingPhone: false
  Rectangle {
    visible: page.confirmingPhone
    Layout.fillWidth: true
    implicitHeight: phoneAsk.implicitHeight + 24
    radius: Theme.radiusS
    color: Theme.surface
    border.width: 1
    border.color: Theme.warn
    ColumnLayout {
      id: phoneAsk
      anchors.fill: parent
      anchors.margins: 12
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: "Pocketdesk lets your phone see and control this computer, and answer FRIDAY's questions. Turning it on starts its server in your session (it starts at every sign-in until you turn it off here). Only phones you pair with a PIN shown here can connect."
        wrapMode: Text.Wrap
        elide: Text.ElideNone
      }
      Row {
        spacing: 8
        Chip { primary: true; text: "Turn it on"; onClicked: { page.confirmingPhone = false; phoneOn.run(); } }
        Chip { text: "Cancel"; onClicked: page.confirmingPhone = false }
      }
    }
  }
  Output { text: phoneOn.output !== "" ? phoneOn.output : phoneOff.output }

  Label { text: "Face unlock"; font.weight: Font.DemiBold; Layout.topMargin: 8 }
  Cmd { id: faceStatus; args: ["face", "status"] }
  Label {
    Layout.fillWidth: true
    text: faceStatus.output !== "" ? faceStatus.output : "Checking…"
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }
  Label {
    Layout.fillWidth: true
    text: "Setting it up looks at your face with the camera and changes login settings, so it asks for your password in a terminal: friday face setup"
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }

  Label { text: "System helper"; font.weight: Font.DemiBold; Layout.topMargin: 8 }
  Cmd { id: helperStatus; args: ["helper", "status"] }
  Label {
    Layout.fillWidth: true
    text: helperStatus.output !== "" ? helperStatus.output : "Checking…"
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }
  Label {
    Layout.fillWidth: true
    text: "With the helper on, FRIDAY can update the system and install or remove packages, and every action asks for your approval in a system prompt. Turn it on or off in a terminal: friday helper enable"
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }
}
