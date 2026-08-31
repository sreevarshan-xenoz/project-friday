import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

// FRIDAY context chooser (R5-2-5-3-1) — a minimal hotkey-summoned dialog that
// asks the user for explicit permission to share the current Screen with the
// FRIDAY assistant.
//
// This is FRIDAY-owned and lives OUTSIDE the frozen omarchy tree, delivered as
// a third-party plugin drop-in to ~/.config/omarchy/plugins/friday.context.
// It deliberately imports only Quickshell core + QtQuick, never qs.Ui/qs.Commons
// (first-party module namespaces a third-party plugin cannot rely on) — exactly
// the discipline the shipped friday.shell plugin follows.
//
// SECURITY CONTRACT: this QML is NOT a security boundary. It only expresses the
// user's intent (Share was pressed). The Rust path (ContextGate → EgressGate →
// provider) stays authoritative and still enforces the `consent: true` that
// `friday-ctl context screen --yes` sends. Editing this QML can never weaken
// the model — it can only fail to express consent.
Item {
  id: root

  property var shell: null
  property var manifest: null

  property bool opened: false

  property color cardBackground: "#1F1F1F"
  property color cardBorder: "#3A3A3A"
  property color cardAccent: "#4C8BF5"
  property color cardText: "#E6E6E6"
  property color cardDim: "#9E9E9E"
  property int cardWidth: 420
  property int cardHeight: 240
  property int cardRadius: 14
  property int contentMargin: 20
  property int contentSpacing: 14

  function open(payloadJson) {
    root.opened = true
  }

  function close() {
    root.opened = false
  }

  function dismiss() {
    root.opened = false
    if (root.shell && typeof root.shell.hide === "function")
      root.shell.hide((root.manifest && root.manifest.id) || "friday.context")
  }

  function share() {
    root.dismiss()
    // The user pressed Share. Consent is collected here in the UI and conveyed
    // as the per-use `consent: true` the frozen seam requires; the ContextGate /
    // EgressGate still enforce it before anything reaches the provider.
    Quickshell.execDetached(["friday-ctl", "context", "screen", "--yes"])
  }

  PanelWindow {
    id: panel
    visible: root.opened
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "friday-context"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
    exclusionMode: ExclusionMode.Ignore

    Rectangle {
      anchors.fill: parent
      color: "#99000000"
    }

    MouseArea {
      anchors.fill: parent
      onClicked: root.dismiss()
    }

    Rectangle {
      id: card
      width: root.cardWidth
      height: root.cardHeight
      radius: root.cardRadius
      anchors.centerIn: parent
      color: root.cardBackground
      border.color: root.cardBorder
      border.width: 1

      MouseArea { anchors.fill: parent; onClicked: {} }

      ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.contentMargin
        spacing: root.contentSpacing

        Text {
          text: "FRIDAY"
          font.pixelSize: 18
          font.bold: true
          color: root.cardAccent
        }

        Text {
          text: "Share your screen with the FRIDAY assistant?"
          font.pixelSize: 14
          color: root.cardText
          wrapMode: Text.WordWrap
          Layout.fillWidth: true
        }

        Text {
          text: "This will send a screenshot to the selected provider."
          font.pixelSize: 12
          color: root.cardDim
          wrapMode: Text.WordWrap
          Layout.fillWidth: true
        }

        Item {
          Layout.fillHeight: true
        }

        RowLayout {
          spacing: root.contentSpacing
          Layout.alignment: Qt.AlignRight

          Button {
            text: "Cancel"
            implicitHeight: 32
            implicitWidth: 84
            onClicked: root.dismiss()
          }

          Button {
            text: "Share"
            implicitHeight: 32
            implicitWidth: 84
            highlighted: true
            onClicked: root.share()
          }
        }
      }
    }
  }
}
