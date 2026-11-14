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
    color: Theme.bg
    border.width: 1
    border.color: Answer.ok ? Theme.outline : Theme.error
    clip: true
    opacity: Answer.visible ? 1 : Math.min(1, height / 40)
    property bool followUpOpen: false

    HoverHandler { id: hover }

    // Put the answer away after a quiet minute; never while it's being read.
    Timer {
      interval: Answer.ok ? 60000 : 90000
      running: win.answering && !hover.hovered && !followUp.activeFocus
      onTriggered: Answer.dismiss()
    }
    Connections {
      target: Answer
      function onSerialChanged() { card.followUpOpen = false; followUp.text = ""; }
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
          size: 20
          mode: win.answering ? (Answer.ok ? "ready" : "stopped") : "working"
        }
        Label {
          Layout.fillWidth: true
          text: win.answering ? Answer.goal
            : Answer.state === "listening" ? "Listening… let go of Super+Y to ask"
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
          onClicked: Answer.dismiss()
        }
        IconButton {
          visible: Answer.state === "working"
          icon: Icons.stop
          size: 26
          tooltip: "Stop"
          onClicked: Friday.cancel()
        }
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

        Chip { visible: Answer.ok; icon: Icons.copy; text: "Copy"; onClicked: Answer.copy() }
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
      }

      Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 40
        visible: card.followUpOpen
        radius: height / 2
        color: Theme.surface
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
          Keys.onEscapePressed: Answer.dismiss()

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
