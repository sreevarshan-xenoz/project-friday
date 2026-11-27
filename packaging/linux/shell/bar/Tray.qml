import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Services.SystemTray
import Quickshell.Widgets
import qs.common
import qs.services

// Status-notifier icons. Left click activates, right click opens the item's
// own menu under the bar.
RowLayout {
  id: root

  spacing: 6
  visible: SystemTray.items.values.length > 0

  Repeater {
    model: SystemTray.items
    Item {
      id: entry
      required property SystemTrayItem modelData
      implicitWidth: 18
      implicitHeight: 18

      IconImage {
        anchors.fill: parent
        source: entry.modelData.icon
        asynchronous: true
      }

      QsMenuAnchor {
        id: menu
        menu: entry.modelData.menu
        anchor.item: entry
        anchor.edges: Edges.Bottom
        anchor.gravity: Edges.Bottom
        anchor.margins.top: 8
      }
      // An open menu keeps an auto-hiding bar up.
      Binding { when: menu.visible; target: Shell; property: "menuOpen"; value: true }

      MouseArea {
        anchors.fill: parent
        anchors.margins: -3
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        cursorShape: Qt.PointingHandCursor
        onClicked: m => {
          if (m.button === Qt.RightButton || (m.button === Qt.LeftButton && entry.modelData.onlyMenu)) {
            if (entry.modelData.hasMenu)
              menu.open();
          } else if (m.button === Qt.MiddleButton) {
            entry.modelData.secondaryActivate();
          } else {
            entry.modelData.activate();
          }
        }
      }
    }
  }
}
