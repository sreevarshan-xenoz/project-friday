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
// step aside while a right-hand drawer is open. One that goes fades and
// slides out, then the ones below close the gap.
PanelWindow {
  id: win

  required property ShellScreen modelData
  screen: modelData
  visible: shown.length > 0 && Shell.focusedScreen() === modelData.name
    && !((Shell.drawer === "control" || Shell.drawer === "session") && Shell.screen === modelData.name)

  anchors { top: true; right: true }
  // Below the bar, or from the top edge when the bar is a bottom dock.
  margins {
    top: Theme.barAtBottom ? Theme.gap + 4 : Theme.barReach + Theme.gap
    right: Math.max(Theme.gap + 4, Theme.barInset)
  }
  implicitWidth: 380
  // A fixed height: resizing the surface as cards come and go blanks it for a
  // frame or two. Only the cards take clicks (the mask).
  implicitHeight: Math.round(modelData.height * 0.75)
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-notifications"
  WlrLayershell.layer: WlrLayer.Overlay
  mask: Region { item: stack }

  // The popups on show: Notifs' list plus any still playing their exit.
  property var shown: []
  function sync() {
    const next = shown.slice();
    Notifs.popups.forEach((p, i) => {
      if (next.indexOf(p) < 0)
        i === 0 ? next.unshift(p) : next.push(p);
    });
    shown = next;
  }
  function gone(n) { shown = shown.filter(p => p !== n); }
  Connections { target: Notifs; function onPopupsChanged() { win.sync(); } }
  Component.onCompleted: sync()

  ColumnLayout {
    id: stack
    width: parent.width
    spacing: Theme.gap

    Repeater {
      // ScriptModel keeps the cards that stay when one comes or goes.
      model: ScriptModel { values: win.shown }
      NotificationCard {
        id: card
        required property var modelData
        Layout.fillWidth: true
        notification: modelData
        // Shrinks to nothing at the end of its exit so the stack closes up.
        property real fold: 1
        Layout.preferredHeight: implicitHeight * fold
        readonly property bool leaving: Notifs.popups.indexOf(modelData) < 0
        onLeavingChanged: if (leaving) { enter.stop(); exit.start(); }

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

        // In: a short plain slide from the right. Out: fade and slide the
        // same way, then fold.
        opacity: 0
        transform: Translate { id: shift; x: 16 }
        Component.onCompleted: if (!leaving) enter.start()
        ParallelAnimation {
          id: enter
          NumberAnimation { target: card; property: "opacity"; to: 1; duration: Theme.medium; easing.type: Theme.easing }
          NumberAnimation { target: shift; property: "x"; to: 0; duration: Theme.medium; easing.type: Theme.easing }
        }
        SequentialAnimation {
          id: exit
          ParallelAnimation {
            NumberAnimation { target: card; property: "opacity"; to: 0; duration: Theme.fast + 20; easing.type: Easing.InCubic }
            NumberAnimation { target: shift; property: "x"; to: 24; duration: Theme.fast + 20; easing.type: Easing.InCubic }
          }
          NumberAnimation { target: card; property: "fold"; to: 0; duration: Theme.fast; easing.type: Theme.easing }
          ScriptAction { script: win.gone(card.modelData) }
        }
      }
    }
  }
}
