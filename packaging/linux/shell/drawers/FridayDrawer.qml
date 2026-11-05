import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components
import qs.services

// FRIDAY's own drawer: what it is doing, a place to ask, and what it said
// last. Opening it sends nothing; a question goes out only on Enter or Send.
FocusScope {
  id: root

  signal done()

  implicitWidth: 460
  implicitHeight: column.implicitHeight

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 14

    // Header: the orb, big, with the status beside it.
    RowLayout {
      Layout.fillWidth: true
      spacing: 14
      Orb { size: 40 }
      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Label {
          text: "FRIDAY"
          font.pixelSize: Theme.fontL
          font.weight: Font.Bold
          font.letterSpacing: 2
        }
        Label {
          Layout.fillWidth: true
          text: Friday.headline
          color: Friday.mode === "working" ? Theme.accent
            : Friday.mode === "ready" ? Theme.subtext : Theme.warn
        }
      }
      Chip {
        visible: Friday.mode === "offline" || Friday.mode === "stopped"
        icon: Icons.restart
        text: "Start"
        primary: true
        onClicked: Friday.startDaemon()
      }
      Chip {
        visible: Friday.working
        icon: Icons.stop
        text: "Stop"
        onClicked: Friday.cancel()
      }
    }

    // Phase track while a goal runs: one segment per phase, lit up to now.
    ColumnLayout {
      Layout.fillWidth: true
      visible: Friday.working
      spacing: 8
      Label {
        Layout.fillWidth: true
        text: Friday.goal
        wrapMode: Text.Wrap
        maximumLineCount: 3
        elide: Text.ElideRight
      }
      RowLayout {
        Layout.fillWidth: true
        spacing: 4
        Repeater {
          model: Friday.phases
          Rectangle {
            required property int index
            Layout.fillWidth: true
            implicitHeight: 4
            radius: 2
            color: index < Friday.phaseIndex ? Theme.accent
              : index === Friday.phaseIndex ? Theme.signal : Theme.overlay
            Behavior on color { ColorAnimation { duration: Theme.medium } }
          }
        }
      }
      // The answer as it arrives; the newest words stay in view.
      Label {
        Layout.fillWidth: true
        visible: Friday.partial !== ""
        text: Friday.partial.length > 400 ? "…" + Friday.partial.slice(-399) : Friday.partial
        wrapMode: Text.Wrap
        maximumLineCount: 8
        color: Theme.text
        font.pixelSize: Theme.fontS + 1
      }
    }

    // Ask.
    Rectangle {
      Layout.fillWidth: true
      implicitHeight: 48
      radius: height / 2
      color: Theme.surface
      border.width: 1
      border.color: input.activeFocus ? Theme.alpha(Theme.accent, 0.6) : Theme.outline

      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 18
        anchors.rightMargin: 6
        spacing: 8

        TextInput {
          id: input
          Layout.fillWidth: true
          focus: true
          color: Theme.text
          selectionColor: Theme.alpha(Theme.accent, 0.4)
          font.family: Theme.sans
          font.pixelSize: Theme.fontM + 1
          clip: true
          onAccepted: root.send()

          Label {
            anchors.verticalCenter: parent.verticalCenter
            visible: input.text === ""
            text: Friday.running ? "Ask FRIDAY anything…" : "Ask FRIDAY (it will start the daemon)…"
            color: Theme.faint
            font.pixelSize: Theme.fontM + 1
          }
        }
        IconButton {
          icon: Icons.send
          size: 36
          iconSize: 18
          active: input.text.trim() !== ""
          onClicked: root.send()
        }
      }
    }

    // One-press questions about what's in front of you.
    RowLayout {
      Layout.fillWidth: true
      spacing: 8
      Chip { icon: Icons.selection; text: "Selection"; onClicked: { Friday.askSelection(); root.done(); } }
      Chip { icon: Icons.screen; text: "Screen"; onClicked: { Friday.askScreen(); root.done(); } }
      Item { Layout.fillWidth: true }
      IconButton { icon: Icons.history; size: 30; iconSize: 16; onClicked: { Friday.answers(); root.done(); } }
      IconButton { icon: Icons.doctor; size: 30; iconSize: 16; onClicked: { Friday.doctor(); root.done(); } }
    }

    // The last answer.
    Rectangle {
      Layout.fillWidth: true
      visible: Friday.lastGoal !== "" || Friday.lastDetail !== ""
      implicitHeight: last.implicitHeight + 28
      radius: Theme.radiusS + 4
      color: Theme.surface

      ColumnLayout {
        id: last
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 14 }
        spacing: 6
        RowLayout {
          Layout.fillWidth: true
          spacing: 8
          Rectangle {
            implicitWidth: outcome.implicitWidth + 14
            implicitHeight: 20
            radius: 10
            color: Theme.alpha(Friday.lastFailed ? Theme.error : Theme.ok, 0.18)
            Label {
              id: outcome
              anchors.centerIn: parent
              text: Friday.lastOutcome || "answer"
              font.pixelSize: Theme.fontS
              font.weight: Font.DemiBold
              color: Friday.lastFailed ? Theme.error : Theme.ok
            }
          }
          Label {
            Layout.fillWidth: true
            text: Friday.lastGoal
            color: Theme.subtext
            font.pixelSize: Theme.fontS + 1
          }
          IconButton {
            visible: Friday.lastDetail !== ""
            icon: Icons.copy
            size: 26
            iconSize: 14
            onClicked: Friday.copy(Friday.lastDetail)
          }
        }
        Label {
          Layout.fillWidth: true
          visible: text !== ""
          text: Friday.lastDetail
          wrapMode: Text.Wrap
          maximumLineCount: 8
          elide: Text.ElideRight
          textFormat: Text.PlainText
        }
      }
    }
  }

  function send() {
    const q = input.text.trim();
    if (q === "")
      return;
    Friday.ask(q);
    input.text = "";
    root.done();
  }
}
