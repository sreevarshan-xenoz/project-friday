import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Services.Notifications
import Quickshell.Widgets
import qs.common
import qs.services

// One notification. A strip on the left carries its weight: ember for
// FRIDAY, red for critical, quiet otherwise. FRIDAY's answers get the orb, a
// copy button and room for a longer body. Clicking the card runs its default
// action; the cross dismisses it.
Rectangle {
  id: root

  required property var notification
  property bool compact: false
  signal closed()

  readonly property bool fromFriday: Notifs.isFriday(notification)
  readonly property bool critical: notification && notification.urgency === NotificationUrgency.Critical
  readonly property color stripe: critical ? Theme.error : fromFriday ? Theme.accent : Theme.overlay
  readonly property var actions: notification ? notification.actions.filter(a => a.identifier !== "default") : []
  readonly property var defaultAction: notification ? notification.actions.find(a => a.identifier === "default") : undefined

  implicitHeight: body.implicitHeight + 24
  radius: 14
  color: compact ? Theme.glassWell : Theme.glass
  border.width: compact ? 0 : 1
  border.color: fromFriday ? Theme.alpha(Theme.accent, 0.45) : Theme.glassEdge

  Rectangle {
    anchors { left: parent.left; top: parent.top; bottom: parent.bottom; margins: 10 }
    width: 3
    radius: 2
    color: root.stripe
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: root.defaultAction ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: if (root.defaultAction) root.defaultAction.invoke()
  }

  ColumnLayout {
    id: body
    anchors { left: parent.left; right: parent.right; top: parent.top }
    anchors.leftMargin: 24
    anchors.rightMargin: 12
    anchors.topMargin: 12
    spacing: 6

    RowLayout {
      Layout.fillWidth: true
      spacing: 8

      Orb { visible: root.fromFriday; size: 16 }
      IconImage {
        visible: !root.fromFriday && source != ""
        implicitSize: 16
        source: root.notification ? AppIcons.source(root.notification.appIcon) : ""
        asynchronous: true
      }
      Label {
        Layout.fillWidth: true
        // Omarchy's own notices ("omarchy-action") are FRIDAY OS's to the person.
        text: !root.notification ? ""
          : /^omarchy/i.test(root.notification.appName || "") ? "FRIDAY OS"
          : (root.notification.appName || "Notification")
        font.pixelSize: Theme.fontS
        font.weight: Font.DemiBold
        color: root.fromFriday ? Theme.accent : Theme.subtext
      }
      IconButton {
        visible: root.fromFriday && root.notification && root.notification.body !== ""
        icon: Icons.copy
        size: 22
        iconSize: 13
        onClicked: Friday.copy(root.notification.body)
      }
      IconButton {
        icon: Icons.close
        size: 22
        iconSize: 13
        onClicked: {
          Notifs.dismiss(root.notification);
          root.closed();
        }
      }
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: 10
      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Label {
          Layout.fillWidth: true
          text: root.notification ? root.notification.summary : ""
          font.weight: Font.DemiBold
          wrapMode: Text.Wrap
          maximumLineCount: 2
        }
        Label {
          Layout.fillWidth: true
          visible: text !== ""
          // StyledText treats "\n" as a space; the spec allows newlines in
          // bodies (FRIDAY's answers end with a "via …" line).
          text: root.notification ? root.notification.body.replace(/\n/g, "<br>") : ""
          textFormat: Text.StyledText
          wrapMode: Text.Wrap
          maximumLineCount: root.compact ? 3 : root.fromFriday ? 10 : 4
          color: Theme.subtext
          font.pixelSize: Theme.fontS + 1
          linkColor: Theme.signal
          onLinkActivated: link => Qt.openUrlExternally(link)
        }
      }
      ClippingRectangle {
        visible: root.notification && root.notification.image !== ""
        implicitWidth: 48
        implicitHeight: 48
        radius: 10
        color: "transparent"
        Image {
          anchors.fill: parent
          source: root.notification ? root.notification.image : ""
          fillMode: Image.PreserveAspectCrop
          asynchronous: true
        }
      }
    }

    Flow {
      Layout.fillWidth: true
      Layout.topMargin: 2
      visible: root.actions.length > 0
      spacing: 6
      Repeater {
        model: root.actions
        Chip {
          required property var modelData
          required property int index
          text: modelData.text || modelData.identifier
          primary: index === 0 && root.fromFriday
          implicitHeight: 28
          onClicked: modelData.invoke()
        }
      }
    }
  }
}
