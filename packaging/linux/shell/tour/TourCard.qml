import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The tour's card, at the bottom middle of the focused screen (above the
// dock), clear of the answer pill. Never takes the keyboard: the tour is
// about pressing FRIDAY's keys in whatever app you're in.
PanelWindow {
  id: win

  required property ShellScreen modelData
  screen: modelData
  visible: Tour.active && Shell.focusedScreen() === modelData.name

  anchors { bottom: true }
  margins.bottom: Theme.barAtBottom ? Theme.barReach + Theme.gap * 3 : Theme.gap * 4
  implicitWidth: 520
  implicitHeight: card.implicitHeight
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-tour"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

  Rectangle {
    id: card
    width: parent.width
    implicitHeight: content.implicitHeight + 2 * Theme.pad
    radius: Theme.radius
    color: Theme.glass
    border.width: 1
    border.color: Theme.alpha(Theme.accent, 0.5)

    ColumnLayout {
      id: content
      x: Theme.pad
      y: Theme.pad
      width: parent.width - 2 * Theme.pad
      spacing: 10

      RowLayout {
        Layout.fillWidth: true
        spacing: 10
        Orb { size: 22 }
        Label {
          Layout.fillWidth: true
          text: Tour.current.title
          font.pixelSize: Theme.fontL
          font.weight: Font.DemiBold
        }
        // Where you are in the tour.
        Row {
          spacing: 5
          Repeater {
            model: Tour.steps.length
            Rectangle {
              required property int index
              width: index === Tour.step ? 16 : 6
              height: 6
              radius: 3
              color: index <= Tour.step ? Theme.accent : Theme.overlay
              Behavior on width { NumberAnimation { duration: Theme.fast } }
            }
          }
        }
      }

      // The keys, drawn as keycaps.
      Row {
        visible: Tour.current.keys !== ""
        spacing: 6
        Repeater {
          model: Tour.current.keys.split("+")
          Rectangle {
            required property string modelData
            width: keyLabel.implicitWidth + 18
            height: 28
            radius: 6
            color: Theme.overlay
            border.width: 1
            border.color: Theme.alpha(Theme.accent, 0.5)
            Label {
              id: keyLabel
              anchors.centerIn: parent
              text: parent.modelData.trim()
              font.weight: Font.DemiBold
            }
          }
        }
      }

      Label {
        Layout.fillWidth: true
        text: Tour.current.body
        wrapMode: Text.Wrap
        elide: Text.ElideNone
        color: Theme.subtext
      }

      Label {
        Layout.fillWidth: true
        visible: Tour.last && Tour.freeModels
        text: "You're on free models (OpenCode Zen, then Kilo or OVHcloud), which may keep what you send. For private work, connect your own account or key."
        wrapMode: Text.Wrap
        elide: Text.ElideNone
        color: Theme.warn
        font.pixelSize: Theme.fontS + 1
      }

      RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Chip {
          visible: Tour.last && Tour.freeModels
          icon: Icons.settings
          text: "Connect an account"
          onClicked: Quickshell.execDetached(["foot", "-T", "FRIDAY · connect an account", "bash", "-c",
            "friday connect; echo; read -rsn1 -p 'Press any key to close.'"])
        }
        Chip {
          visible: Tour.current.action !== undefined
          icon: Icons.settings
          text: Tour.current.action ? Tour.current.action.label : ""
          onClicked: Quickshell.execDetached(["foot", "-T", "FRIDAY · " + Tour.current.action.label, "bash", "-c",
            Tour.current.action.command + "; echo; read -rsn1 -p 'Press any key to close.'"])
        }
        Item { Layout.fillWidth: true }
        Chip { visible: Tour.step > 0; text: "Back"; onClicked: Tour.back() }
        Chip { visible: !Tour.last; text: "Skip tour"; onClicked: Tour.finish() }
        Chip {
          primary: true
          text: Tour.last ? "Done" : (Tour.current.waits ? "Skip this step" : "Next")
          onClicked: Tour.next()
        }
      }
    }
  }
}
