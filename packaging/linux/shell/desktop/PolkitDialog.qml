import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Polkit
import qs.common
import qs.components
import qs.services

// FRIDAY's polkit agent: when an app asks for an administrator's password
// (installing, mounting, changing a system setting), this card asks for it.
// Loaded only when shell.json has "polkit": true (FRIDAY OS sets it; see
// shell.qml): elsewhere your desktop's own agent keeps the job, since only
// one agent can register.
Scope {
  id: root

  readonly property var flow: agent.flow
  readonly property bool asking: flow !== null && !flow.isCompleted

  function cancel() {
    input.text = "";
    if (root.flow) root.flow.cancelAuthenticationRequest();
  }

  // polkit's own wording ("Authentication is needed to run `/usr/bin/x' as
  // the super user") in plain words; anything else as it comes.
  function plain(message) {
    const m = /^Authentication is needed to run [`'"](.+?)['"] as the super user$/.exec(message.trim());
    return m ? "An app wants to run " + m[1] + " as administrator." : message;
  }

  function send() {
    if (!root.flow || !root.flow.isResponseRequired || input.text === "") return;
    const answer = input.text;
    input.text = "";
    root.flow.submit(answer);
  }

  PolkitAgent {
    id: agent
    path: "/org/friday/PolkitAgent"
  }

  PanelWindow {
    id: win
    visible: root.asking
    screen: Quickshell.screens.find(s => s.name === Shell.focusedScreen()) || Quickshell.screens[0] || null

    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: Theme.alpha(Theme.bg, 0.45)
    WlrLayershell.namespace: "friday-polkit"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: root.asking ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    // The password never outlives the prompt: cleared on showing, on
    // sending it, and on closing.
    onVisibleChanged: { input.text = ""; if (visible) input.forceActiveFocus(); }

    // Escape cancels wherever the focus is (the card takes the keyboard).
    Shortcut {
      sequence: "Escape"
      enabled: win.visible
      onActivated: root.cancel()
    }

    Rectangle {
      id: card
      anchors.centerIn: parent
      width: 420
      implicitHeight: content.implicitHeight + 2 * Theme.pad
      radius: Theme.radius
      color: Theme.glass
      border.width: 1
      border.color: Theme.alpha(Theme.accent, 0.5)

      // Comes in with a short fade and rise; shakes on a wrong password
      // (long enough to read as a shake, not a flicker).
      opacity: 0
      transform: [Translate { id: shake }, Translate { id: rise }]
      Connections {
        target: win
        function onVisibleChanged() { if (win.visible) appear.restart(); else card.opacity = 0; }
      }
      ParallelAnimation {
        id: appear
        NumberAnimation { target: card; property: "opacity"; from: 0; to: 1; duration: Theme.medium; easing.type: Theme.easing }
        NumberAnimation { target: rise; property: "y"; from: 12; to: 0; duration: Theme.medium; easing.type: Theme.easing }
      }
      SequentialAnimation {
        id: shakeAnim
        NumberAnimation { target: shake; property: "x"; to: -10; duration: Theme.motion ? 60 : 0; easing.type: Easing.OutQuad }
        NumberAnimation { target: shake; property: "x"; to: 10; duration: Theme.motion ? 80 : 0; easing.type: Easing.InOutQuad }
        NumberAnimation { target: shake; property: "x"; to: -5; duration: Theme.motion ? 70 : 0; easing.type: Easing.InOutQuad }
        NumberAnimation { target: shake; property: "x"; to: 0; duration: Theme.motion ? 60 : 0; easing.type: Easing.OutQuad }
      }
      Connections {
        target: root.flow
        ignoreUnknownSignals: true
        function onAuthenticationFailed() { input.text = ""; shakeAnim.restart(); }
      }

      ColumnLayout {
        id: content
        x: Theme.pad
        y: Theme.pad
        width: parent.width - 2 * Theme.pad
        spacing: 12

        RowLayout {
          spacing: 10
          Orb { size: 22 }
          Label {
            Layout.fillWidth: true
            text: "Administrator password"
            font.pixelSize: Theme.fontL
            font.weight: Font.DemiBold
          }
        }
        Label {
          Layout.fillWidth: true
          text: root.flow ? root.plain(root.flow.message) : ""
          wrapMode: Text.Wrap
          elide: Text.ElideNone
          color: Theme.subtext
        }
        Rectangle {
          Layout.fillWidth: true
          implicitHeight: 40
          radius: Theme.radiusS
          color: Theme.glassWell
          border.width: 1
          // Red after a wrong password until something new is typed.
          border.color: root.flow && root.flow.failed && input.text === "" ? Theme.error
            : input.activeFocus ? Theme.accent : Theme.glassEdge
          TextInput {
            id: input
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            verticalAlignment: TextInput.AlignVCenter
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: Theme.fontM + 1
            echoMode: root.flow && root.flow.responseVisible ? TextInput.Normal : TextInput.Password
            enabled: root.flow !== null && root.flow.isResponseRequired
            clip: true
            onAccepted: root.send()
          }
          Label {
            anchors.fill: parent
            anchors.leftMargin: 12
            visible: input.text === ""
            text: root.flow && root.flow.inputPrompt ? root.flow.inputPrompt.replace(/:\s*$/, "") : "Password"
            color: Theme.faint
          }
        }
        Label {
          Layout.fillWidth: true
          visible: text !== ""
          text: !root.flow ? "" : root.flow.supplementaryMessage !== "" ? root.flow.supplementaryMessage
            : root.flow.failed ? "That password didn't work. Try again." : ""
          wrapMode: Text.Wrap
          elide: Text.ElideNone
          color: root.flow && (root.flow.supplementaryIsError || root.flow.failed) ? Theme.error : Theme.subtext
        }
        RowLayout {
          Layout.alignment: Qt.AlignRight
          spacing: 8
          Chip { text: "Cancel"; onClicked: root.cancel() }
          Chip {
            text: "Allow"
            primary: true
            // Nothing to send until a password is typed.
            enabled: input.text !== ""
            opacity: enabled ? 1 : 0.45
            onClicked: root.send()
          }
        }
      }
    }
  }
}
