import QtQuick
import QtQuick.Layouts
import Quickshell.Bluetooth
import Quickshell.Io
import Quickshell.Services.UPower
import qs.common
import qs.components
import qs.services

// Network, Bluetooth, sound and battery in one pill (and a mark while
// recall keeps window text, `friday recall`). Click opens the control
// drawer; scroll changes the volume.
Rectangle {
  id: root

  signal clicked()
  // Stacked top to bottom (the rail style).
  property bool vertical: false

  readonly property var adapter: Bluetooth.defaultAdapter
  readonly property var battery: UPower.displayDevice
  readonly property bool hasBattery: battery !== null && battery.ready && battery.isLaptopBattery
  readonly property bool charging: hasBattery && battery.state === UPowerDeviceState.Charging
  readonly property int pct: hasBattery ? Math.round(battery.percentage * 100) : 0

  implicitWidth: vertical ? Math.max(28, row.implicitWidth + 8) : row.implicitWidth + 22
  implicitHeight: vertical ? row.implicitHeight + 20 : 24
  radius: height / 2
  color: mouse.containsMouse ? Theme.raised : Theme.surface
  Behavior on color { ColorAnimation { duration: Theme.fast } }

  // Whether recall is keeping window text, or the wake word is listening
  // (their user services are running).
  property bool recalling: false
  property bool listening: false
  Process {
    id: recallCheck
    command: ["systemctl", "--user", "is-active", "--quiet", "friday-recall.service"]
    onExited: code => root.recalling = code === 0
  }
  // The microphone is in use: the wake word, or captions of the microphone.
  Process {
    id: wakeCheck
    command: ["sh", "-c", "systemctl --user is-active --quiet friday-wake.service || pgrep -f '[f]riday-ctl listen captions --mic' >/dev/null"]
    onExited: code => root.listening = code === 0
  }
  Timer {
    interval: 10000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: { recallCheck.running = true; wakeCheck.running = true; }
  }

  GridLayout {
    id: row
    anchors.centerIn: parent
    flow: root.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
    rows: root.vertical ? -1 : 1
    columns: root.vertical ? 1 : -1
    rowSpacing: 10
    columnSpacing: 10

    Glyph {
      visible: root.listening
      text: Icons.mic
      size: 15
      color: Theme.warn
    }
    Glyph {
      visible: root.recalling
      text: Icons.history
      size: 15
      color: Theme.accent
    }

    Glyph {
      visible: Notifs.dnd
      text: Icons.bellOff
      size: 15
      color: Theme.warn
    }
    Glyph {
      visible: Network.ready
      text: Network.icon()
      size: 15
      color: Network.connected ? Theme.text : Theme.faint
    }
    Glyph {
      visible: root.adapter !== null
      text: root.adapter && root.adapter.enabled
        ? (root.adapter.devices.values.some(d => d.connected) ? Icons.bluetoothConnected : Icons.bluetooth)
        : Icons.bluetoothOff
      size: 15
      color: root.adapter && root.adapter.enabled ? Theme.text : Theme.faint
    }
    Glyph {
      visible: Audio.ready
      text: Audio.icon()
      size: 15
      color: Audio.muted ? Theme.faint : Theme.text
    }
    Glyph {
      visible: !Notifs.dnd && !Network.ready && root.adapter === null && !Audio.ready && !root.hasBattery
      text: Icons.settings
      size: 15
    }
    GridLayout {
      visible: root.hasBattery
      flow: root.vertical ? GridLayout.TopToBottom : GridLayout.LeftToRight
      rows: root.vertical ? -1 : 1
      columns: root.vertical ? 1 : -1
      rowSpacing: 1
      columnSpacing: 3
      Glyph {
        text: root.charging ? Icons.batteryCharging : root.pct > 60 ? Icons.battery
          : root.pct > 20 ? Icons.batteryHalf : Icons.batteryLow
        size: 15
        color: !root.charging && root.pct <= 15 ? Theme.error : Theme.text
      }
      Label {
        text: root.pct + "%"
        font.family: Theme.mono
        font.pixelSize: Theme.fontS
        color: Theme.subtext
      }
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
    onWheel: w => Audio.setVolume(Audio.volume + (w.angleDelta.y > 0 ? 0.05 : -0.05))
  }
}
