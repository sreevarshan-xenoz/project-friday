import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Bluetooth
import Quickshell.Services.Mpris
import Quickshell.Widgets
import qs.common
import qs.components
import qs.services

// Quick settings and the notification history. Tiles toggle (right click on
// Wi-Fi opens network settings); sliders drag or scroll.
FocusScope {
  id: root

  signal done()

  implicitWidth: 380
  implicitHeight: column.implicitHeight

  readonly property var adapter: Bluetooth.defaultAdapter
  readonly property var player: Mpris.players.values.find(p => p.isPlaying) || Mpris.players.values[0] || null

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 12

    GridLayout {
      Layout.fillWidth: true
      columns: 2
      rowSpacing: 8
      columnSpacing: 8

      Tile {
        Layout.fillWidth: true
        visible: Network.ready
        icon: Network.icon()
        title: Network.kind === "ethernet" ? "Wired" : "Wi-Fi"
        status: Network.connected ? Network.name : Network.wifiEnabled ? "Not connected" : "Off"
        on: Network.kind === "ethernet" || Network.wifiEnabled
        onClicked: if (Network.kind !== "ethernet") Network.setWifi(!Network.wifiEnabled)
        onSecondaryClicked: { Network.settings(); root.done(); }
      }
      Tile {
        Layout.fillWidth: true
        visible: root.adapter !== null
        icon: root.adapter && root.adapter.enabled ? Icons.bluetooth : Icons.bluetoothOff
        title: "Bluetooth"
        status: {
          if (!root.adapter || !root.adapter.enabled)
            return "Off";
          const c = root.adapter.devices.values.filter(d => d.connected);
          return c.length === 0 ? "On" : c.length === 1 ? (c[0].name || c[0].deviceName) : c.length + " devices";
        }
        on: root.adapter !== null && root.adapter.enabled
        onClicked: if (root.adapter) root.adapter.enabled = !root.adapter.enabled
      }
      Tile {
        Layout.fillWidth: true
        icon: Notifs.dnd ? Icons.bellOff : Icons.bell
        title: "Do not disturb"
        status: Notifs.dnd ? "On" : "Off"
        on: Notifs.dnd
        onClicked: Notifs.dnd = !Notifs.dnd
      }
      Tile {
        Layout.fillWidth: true
        visible: Audio.micReady
        icon: Audio.micMuted ? Icons.micOff : Icons.mic
        title: "Microphone"
        status: Audio.micMuted ? "Muted" : "On"
        on: !Audio.micMuted
        onClicked: Audio.toggleMic()
      }
    }

    Slider {
      Layout.fillWidth: true
      visible: Audio.ready
      icon: Audio.icon()
      value: Audio.volume
      fill: Audio.muted ? Theme.faint : Theme.accent
      onMoved: v => Audio.setVolume(v)
      onIconClicked: Audio.toggleMute()
    }
    Slider {
      Layout.fillWidth: true
      visible: Brightness.ready
      icon: Icons.brightness
      value: Brightness.value
      fill: Theme.signal
      onMoved: v => Brightness.set(v)
    }

    // Look: the shell's style and palette, applied live (shell.json).
    ColumnLayout {
      Layout.fillWidth: true
      spacing: 6
      Label {
        text: "Look"
        font.pixelSize: Theme.fontS
        font.weight: Font.DemiBold
        color: Theme.subtext
      }
      Flow {
        Layout.fillWidth: true
        spacing: 6
        Repeater {
          model: Config.styles
          Chip {
            required property string modelData
            text: modelData.charAt(0).toUpperCase() + modelData.slice(1)
            primary: Config.style === modelData
            implicitHeight: 28
            // At once here; `friday shell style` also matches Hyprland's
            // window gaps, rounding and borders to it.
            onClicked: {
              Config.set("style", modelData);
              Quickshell.execDetached(["friday", "shell", "style", modelData]);
            }
          }
        }
      }
      Flow {
        Layout.fillWidth: true
        spacing: 6
        Repeater {
          model: Config.palettes
          Chip {
            required property string modelData
            text: modelData.charAt(0).toUpperCase() + modelData.slice(1)
            primary: Config.palette === modelData
            implicitHeight: 28
            // At once here; `friday shell palette` also matches Hyprland's
            // window gaps, rounding and borders to it.
            onClicked: {
              Config.set("palette", modelData);
              Quickshell.execDetached(["friday", "shell", "palette", modelData]);
            }
          }
        }
      }
    }

    // Now playing.
    Rectangle {
      Layout.fillWidth: true
      visible: root.player !== null
      implicitHeight: 64
      radius: Theme.radiusS + 4
      color: Theme.surface

      RowLayout {
        anchors.fill: parent
        anchors.margins: 10
        spacing: 12
        ClippingRectangle {
          implicitWidth: 44
          implicitHeight: 44
          radius: 8
          color: Theme.raised
          Image {
            anchors.fill: parent
            source: root.player ? root.player.trackArtUrl : ""
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
          }
        }
        ColumnLayout {
          Layout.fillWidth: true
          spacing: 0
          Label { Layout.fillWidth: true; text: root.player ? root.player.trackTitle || "Unknown" : ""; font.weight: Font.Medium }
          Label {
            Layout.fillWidth: true
            text: root.player ? root.player.trackArtist || root.player.identity : ""
            font.pixelSize: Theme.fontS
            color: Theme.subtext
          }
        }
        IconButton { icon: Icons.previous; size: 30; onClicked: if (root.player) root.player.previous() }
        IconButton {
          icon: root.player && root.player.isPlaying ? Icons.pause : Icons.play
          size: 34
          active: true
          onClicked: if (root.player) root.player.togglePlaying()
        }
        IconButton { icon: Icons.next; size: 30; onClicked: if (root.player) root.player.next() }
      }
    }

    // Notifications.
    RowLayout {
      Layout.fillWidth: true
      Layout.topMargin: 4
      Label {
        Layout.fillWidth: true
        text: Notifs.list.length > 0 ? "Notifications · " + Notifs.list.length : "No notifications"
        color: Theme.subtext
        font.pixelSize: Theme.fontS
        font.weight: Font.DemiBold
        font.letterSpacing: 1
      }
      IconButton {
        visible: Notifs.list.length > 0
        icon: Icons.clearAll
        size: 28
        iconSize: 15
        onClicked: Notifs.clearAll()
      }
    }

    Flickable {
      Layout.fillWidth: true
      visible: Notifs.list.length > 0
      implicitHeight: Math.min(notifList.implicitHeight, 340)
      contentHeight: notifList.implicitHeight
      clip: true
      boundsBehavior: Flickable.StopAtBounds

      ColumnLayout {
        id: notifList
        width: parent.width
        spacing: 8
        Repeater {
          model: Notifs.list.slice().reverse()
          NotificationCard {
            required property var modelData
            Layout.fillWidth: true
            notification: modelData
            compact: true
          }
        }
      }
    }
  }
}
