import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

// A month at a glance. Today is the ember square; scroll or the chevrons
// change the month.
FocusScope {
  id: root

  signal done()

  implicitWidth: 300
  implicitHeight: column.implicitHeight

  SystemClock { id: clock; precision: SystemClock.Minutes }
  property int offset: 0
  readonly property date today: clock.date
  readonly property date month: new Date(today.getFullYear(), today.getMonth() + offset, 1)

  // Monday-first grid of 42 days covering the month.
  readonly property var days: {
    const first = new Date(month.getFullYear(), month.getMonth(), 1);
    const lead = (first.getDay() + 6) % 7;
    const out = [];
    for (let i = 0; i < 42; i++)
      out.push(new Date(month.getFullYear(), month.getMonth(), 1 - lead + i));
    return out;
  }

  Keys.onLeftPressed: offset--
  Keys.onRightPressed: offset++

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 10

    ColumnLayout {
      spacing: 0
      Label {
        text: Qt.formatDateTime(clock.date, Config.clock24h ? "HH:mm" : "h:mm AP")
        font.family: Theme.mono
        font.pixelSize: 34
        font.weight: Font.Bold
      }
      Label {
        text: Qt.formatDate(clock.date, "dddd d MMMM yyyy")
        color: Theme.subtext
      }
    }

    RowLayout {
      Layout.fillWidth: true
      Label {
        Layout.fillWidth: true
        text: Qt.formatDate(root.month, "MMMM yyyy")
        font.weight: Font.DemiBold
        color: root.offset === 0 ? Theme.text : Theme.accent
        MouseArea { anchors.fill: parent; onClicked: root.offset = 0 }
      }
      IconButton { tooltip: "Previous month"; icon: Icons.chevronLeft; size: 28; onClicked: root.offset-- }
      IconButton { tooltip: "Next month"; icon: Icons.chevronRight; size: 28; onClicked: root.offset++ }
    }

    GridLayout {
      Layout.fillWidth: true
      columns: 7
      rowSpacing: 2
      columnSpacing: 2

      Repeater {
        model: ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        Label {
          required property string modelData
          Layout.fillWidth: true
          horizontalAlignment: Text.AlignHCenter
          text: modelData
          font.pixelSize: Theme.fontS
          color: Theme.faint
        }
      }

      Repeater {
        model: root.days
        Rectangle {
          required property var modelData
          readonly property bool inMonth: modelData.getMonth() === root.month.getMonth()
          readonly property bool isToday: modelData.toDateString() === root.today.toDateString()
          Layout.fillWidth: true
          implicitHeight: 34
          radius: 9
          color: isToday ? Theme.accent : "transparent"
          Label {
            anchors.centerIn: parent
            text: modelData.getDate()
            font.family: Theme.mono
            font.pixelSize: Theme.fontS + 1
            font.weight: parent.isToday ? Font.Bold : Font.Normal
            color: parent.isToday ? Theme.onAccent : parent.inMonth ? Theme.text : Theme.faint
          }
        }
      }
    }
  }

  WheelHandler { onWheel: e => root.offset += e.angleDelta.y > 0 ? -1 : 1 }
}
