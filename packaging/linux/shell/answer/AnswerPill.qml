import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// FRIDAY's answers, in one surface under the middle of the bar (above the
// dock) on the focused screen. It starts as a small pill while FRIDAY works,
// grows as the answer streams in, and opens into a card with the answer and
// what to do with it: copy, type it into the window you were in, ask a
// follow-up, open the full answer, ask again. It springs a little as it grows
// and settles quickly as it shrinks. Esc, the close button or a quiet minute
// (unless the pointer is on it) puts it away.
PanelWindow {
  id: win

  required property ShellScreen modelData
  screen: modelData
  visible: (Answer.visible || card.height > 1) && Shell.focusedScreen() === modelData.name

  anchors { top: !Theme.barAtBottom; bottom: Theme.barAtBottom }
  margins {
    top: Theme.barAtBottom ? 0 : Theme.barReach + Theme.gap
    bottom: Theme.barAtBottom ? Theme.barReach + Theme.gap : 0
  }
  implicitWidth: 720
  implicitHeight: Math.round(modelData.height * 0.7)
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-answer"
  WlrLayershell.layer: WlrLayer.Overlay
  // Focus only on a click, so an answer never takes the keyboard from your app.
  WlrLayershell.keyboardFocus: answering ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
  mask: Region { item: card }

  readonly property bool answering: Answer.state === "answer"
  readonly property bool streaming: Answer.state === "working" && Answer.body !== ""
  readonly property real maxBody: implicitHeight - 150

  // The size it is heading for; `morph` animates there, springing when it
  // grows and easing out quickly when it shrinks.
  readonly property real targetW: !Answer.visible ? 300 : answering ? 640 : streaming ? 560 : 380
  readonly property real targetH: !Answer.visible ? 0 : content.implicitHeight + 2 * 14
  onTargetWChanged: morph(widthAnim, card.width, targetW)
  onTargetHChanged: morph(heightAnim, card.height, targetH)

  function morph(anim, from, to) {
    anim.stop();
    anim.from = from;
    anim.to = to;
    anim.easing.type = to > from ? Easing.OutBack : Easing.OutQuart;
    anim.easing.overshoot = 1.15;
    anim.duration = to > from ? Theme.slow : Theme.medium;
    anim.start();
  }

  NumberAnimation { id: widthAnim; target: card; property: "width" }
  NumberAnimation { id: heightAnim; target: card; property: "height" }

  Rectangle {
    id: card
    width: 300
    height: 0
    anchors.horizontalCenter: parent.horizontalCenter
    y: Theme.barAtBottom ? parent.height - height : 0
    radius: Math.min(Theme.radius + 6, height / 2)
    color: Theme.glass
    border.width: 1
    border.color: Answer.ok ? Theme.glassEdge : Theme.error
    clip: true
    focus: true
    Keys.onEscapePressed: Answer.close()
    opacity: Answer.visible ? 1 : Math.min(1, height / 40)
    property bool followUpOpen: false
    property bool memoriesOpen: false

    HoverHandler { id: hover }

    // Put the answer away after a quiet minute; never while it's being read.
    Timer {
      id: quiet
      interval: Answer.ok ? 60000 : 90000
      running: win.answering && !hover.hovered && !followUp.activeFocus && followUp.text === ""
      onTriggered: Answer.dismiss()
    }
    Connections {
      target: Answer
      function onSerialChanged() { card.followUpOpen = false; card.memoriesOpen = false; followUp.text = ""; quiet.restart(); }
    }

    ColumnLayout {
      id: content
      x: 18
      y: 14
      width: card.width - 36
      spacing: 10
      opacity: card.height > 30 ? 1 : 0
      Behavior on opacity { NumberAnimation { duration: Theme.fast } }

      RowLayout {
        Layout.fillWidth: true
        spacing: 10

        Orb {
          visible: Config.featureOn("status-orb") && win.answering && !Friday.speaking
          size: 20
          mode: Answer.ok ? "ready" : "stopped"
        }
        // Listening, thinking and speaking: ribbons of light that flow like a voice.
        Aura {
          visible: Config.featureOn("status-orb") && (!win.answering || Friday.speaking)
          size: 30
          mode: win.answering ? "speaking" : Answer.state === "listening" ? "listening" : "working"
          primary: Theme.accent
          secondary: Theme.signal
        }
        Label {
          Layout.fillWidth: true
          text: win.answering ? Answer.goal
            : Answer.state === "listening" ? "Listening… " + Answer.listenHint
            : (Answer.goal ? "Working on: " + Answer.goal : "Working…")
          font.pixelSize: Theme.fontM
          font.weight: Font.DemiBold
          color: win.answering && !Answer.ok ? Theme.error : Theme.text
        }
        IconButton {
          visible: win.answering
          icon: Icons.close
          size: 26
          tooltip: "Close (Esc)"
          onClicked: Answer.close()
        }
        IconButton {
          visible: Answer.state === "working"
          icon: Icons.stop
          size: 26
          tooltip: "Stop"
          onClicked: Friday.cancel()
        }
      }

      // What went with the question, before the first word.
      Label {
        Layout.fillWidth: true
        visible: Answer.state === "working" && Answer.receipt !== ""
        text: "Sent: " + Answer.receipt
        color: Theme.subtext
        font.pixelSize: Theme.fontS
        wrapMode: Text.Wrap
      }

      // The answer: the tail while it streams, all of it (scrollable) after.
      Flickable {
        id: flick
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(answerText.implicitHeight, win.answering ? win.maxBody : 140)
        visible: Answer.body !== ""
        contentHeight: answerText.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentY: win.answering ? 0 : Math.max(0, contentHeight - height)

        Text {
          id: answerText
          width: flick.width
          text: Answer.body
          textFormat: win.answering ? Text.MarkdownText : Text.PlainText
          wrapMode: Text.Wrap
          color: Theme.text
          font.family: Theme.sans
          font.pixelSize: Theme.fontM
          lineHeight: 1.15
          onLinkActivated: function(link) { Qt.openUrlExternally(link); }
        }
      }

      Label {
        Layout.fillWidth: true
        visible: win.answering && Answer.meta !== ""
        text: Answer.meta
        color: Theme.subtext
        font.pixelSize: Theme.fontS
        wrapMode: Text.Wrap
        elide: Text.ElideNone
      }

      Flow {
        Layout.fillWidth: true
        visible: win.answering
        spacing: 8

        Chip { visible: Answer.body !== ""; icon: Icons.copy; text: "Copy"; onClicked: Answer.copy() }
        // Each code block in the answer, copied on its own.
        Repeater {
          model: win.answering ? Answer.codeBlocks.length : 0
          Chip {
            required property int index
            icon: Icons.copy
            text: Answer.codeBlocks.length > 1 ? "Copy code " + (index + 1) : "Copy code"
            onClicked: Answer.copyCode(index)
          }
        }
        Chip { visible: Answer.ok; icon: Icons.selection; text: "Type it"; onClicked: Answer.typeIt() }
        Chip {
          visible: Answer.ok
          icon: Icons.send
          text: "Follow up"
          primary: card.followUpOpen
          onClicked: { card.followUpOpen = true; followUp.forceActiveFocus(); }
        }
        Chip { icon: Icons.history; text: "Full answer"; onClicked: Answer.openFull() }
        Chip { icon: Icons.restart; text: "Ask again"; onClicked: { Answer.askAgain(); Answer.dismiss(); } }
        Chip { visible: Answer.goal !== ""; icon: Icons.close; text: "That was wrong"; onClicked: { Answer.markWrong(); Answer.dismiss(); } }
        Chip {
          visible: Answer.ok && Answer.canTryAnother
          icon: Icons.restart
          text: "Try another model"
          onClicked: { Answer.tryAnother(); Answer.dismiss(); }
        }
        Chip {
          // A finished answer's: never the last one's while a new one works.
          visible: win.answering && Answer.memories.length > 0 && Config.featureOn("memories-used")
          icon: Icons.memory
          text: "Used " + Answer.memories.length + (Answer.memories.length === 1 ? " memory" : " memories")
          primary: card.memoriesOpen
          onClicked: { card.memoriesOpen = !card.memoriesOpen; }
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        visible: win.answering && card.memoriesOpen && Answer.memories.length > 0 && Config.featureOn("memories-used")
        spacing: 6

        Repeater {
          model: Answer.memories
          RowLayout {
            required property int index
            required property string modelData
            Layout.fillWidth: true
            spacing: 8

            Label {
              Layout.fillWidth: true
              text: modelData
              color: Theme.subtext
              font.pixelSize: Theme.fontS
              elide: Text.ElideRight
            }

            Chip {
              text: "Forget"
              implicitHeight: 24
              onClicked: Answer.forgetMemory(index)
            }
          }
        }
      }

      Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        visible: card.followUpOpen
        radius: height / 2
        color: Theme.glassWell
        border.width: 1
        border.color: followUp.activeFocus ? Theme.accent : Theme.outline

        TextInput {
          id: followUp
          anchors.fill: parent
          anchors.leftMargin: 16
          anchors.rightMargin: 16
          verticalAlignment: TextInput.AlignVCenter
          color: Theme.text
          font.family: Theme.sans
          font.pixelSize: Theme.fontM
          clip: true
          onAccepted: {
            Answer.followUp(text);
            Answer.dismiss();
          }
          Keys.onEscapePressed: Answer.close()

          Label {
            anchors.fill: parent
            visible: followUp.text === ""
            text: "Ask a follow-up about this answer…"
            color: Theme.faint
          }
        }
      }
    }
  }
}
