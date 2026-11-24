import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components
import qs.services

// FRIDAY Settings: pages on the left, switches and buttons on the right. It
// runs the same `friday` commands a terminal would; opened by `friday app`.
FloatingWindow {
  id: win

  visible: Settings.open
  onVisibleChanged: if (!visible) Settings.open = false
  title: "FRIDAY Settings"
  implicitWidth: 820
  implicitHeight: 560
  minimumSize: Qt.size(640, 420)
  color: Theme.bg

  readonly property var pages: ["Home", "AI", "Voice", "Permissions", "Privacy", "Features", "Keys", "Commands", "Health"]

  RowLayout {
    anchors.fill: parent
    spacing: 0

    Rectangle {
      Layout.fillHeight: true
      Layout.preferredWidth: 190
      color: Theme.surface

      ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 4
        Label { text: "FRIDAY"; color: Theme.accent; font.family: Theme.display; font.pixelSize: Theme.fontL; font.letterSpacing: 5; Layout.bottomMargin: 10 }
        Repeater {
          model: win.pages
          Rectangle {
            required property string modelData
            Layout.fillWidth: true
            implicitHeight: 36
            radius: Theme.radiusS
            color: Settings.page === modelData ? Theme.raised : (hover.containsMouse ? Theme.overlay : "transparent")
            border.width: activeFocus ? 2 : 0
            border.color: Theme.accent
            activeFocusOnTab: true
            Keys.onReturnPressed: Settings.page = modelData
            Keys.onEnterPressed: Settings.page = modelData
            Keys.onSpacePressed: Settings.page = modelData
            Label {
              anchors.verticalCenter: parent.verticalCenter
              anchors.left: parent.left
              anchors.leftMargin: 12
              text: parent.modelData
              color: Settings.page === parent.modelData ? Theme.accent : Theme.text
            }
            MouseArea { id: hover; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: Settings.page = parent.modelData }
          }
        }
        Item { Layout.fillHeight: true }
      }
    }

    Flickable {
      Layout.fillWidth: true
      Layout.fillHeight: true
      contentHeight: pageLoader.implicitHeight + 60
      boundsBehavior: Flickable.StopAtBounds
      clip: true

      Loader {
        id: pageLoader
        x: 28
        y: 24
        width: Math.min(parent.width - 56, 620)
        source: Settings.page + "Page.qml"
      }
    }
  }
}
