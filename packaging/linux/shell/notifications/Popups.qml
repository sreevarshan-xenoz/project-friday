import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Notifications
import qs.common
import qs.components
import qs.services

// New notifications, stacked under the right end of the bar on the focused
// screen. Each one leaves after its timeout (FRIDAY's answers stay three
// times as long, critical ones until dismissed); hovering holds it. They
// step aside while a right-hand drawer is open.
PanelWindow {
  id: win

  required property ShellScreen modelData
  screen: modelData
  visible: Notifs.popups.length > 0 && Shell.focusedScreen() === modelData.name
    && !((Shell.drawer === "control" || Shell.drawer === "session") && Shell.screen === modelData.name)

  anchors { top: true; right: true }
  // Below the bar, or from the top edge when the bar is a bottom dock.
  margins {
    top: Theme.barAtBottom ? Theme.gap + 4 : Theme.barReach + Theme.gap
    right: Math.max(Theme.gap + 4, Theme.barInset)
  }
  implicitWidth: 380
  implicitHeight: Math.max(1, stack.implicitHeight)
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-notifications"
  WlrLayershell.layer: WlrLayer.Overlay
  mask: Region { item: stack }

  ColumnLayout {
    id: stack
    width: parent.width
    spacing: Theme.gap

    Repeater {
      model: Notifs.popups
      NotificationCard {
        id: card
        required property var modelData
        Layout.fillWidth: true
        notification: modelData

        readonly property int timeout: {
          if (critical)
            return 0;
          if (modelData.expireTimeout > 0)
            return modelData.expireTimeout;
          return Config.notificationTimeout * 1000 * (fromFriday ? 3 : 1);
        }

        HoverHandler { id: hover }
        Timer {
          interval: Math.max(1000, card.timeout)
          running: card.timeout > 0 && !hover.hovered
          onTriggered: Notifs.hidePopup(card.modelData)
        }

        opacity: 0
        scale: 0.96
        transform: Translate { id: shift; x: 40 }
        Component.onCompleted: enter.start()
        ParallelAnimation {
          id: enter
          NumberAnimation { target: card; property: "opacity"; to: 1; duration: Theme.medium; easing.type: Theme.easing }
          NumberAnimation { target: shift; property: "x"; to: 0; duration: Theme.slow; easing.type: Theme.spring; easing.overshoot: 1.3 }
          NumberAnimation { target: card; property: "scale"; to: 1; duration: Theme.slow; easing.type: Theme.spring }
        }
      }
    }
  }
}
