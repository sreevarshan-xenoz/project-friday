import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Widgets
import qs.common
import qs.components
import qs.services

// Find an app and start it, or hand the words to FRIDAY. Type to filter,
// arrows to move, Enter to launch. The last row is always "Ask FRIDAY".
FocusScope {
  id: root

  signal done()

  implicitWidth: 420
  implicitHeight: column.implicitHeight

  property int selected: 0
  readonly property string query: input.text.trim().toLowerCase()

  readonly property var results: {
    const q = query;
    const apps = DesktopEntries.applications.values.filter(e => !e.noDisplay);
    if (q === "")
      return apps.slice().sort((a, b) => a.name.localeCompare(b.name)).slice(0, 7);
    const scored = [];
    for (const e of apps) {
      const name = (e.name || "").toLowerCase();
      const extra = [e.genericName, e.comment, (e.keywords || []).join(" ")].join(" ").toLowerCase();
      let s = 0;
      if (name.startsWith(q)) s = 4;
      else if (name.split(/[\s\-_.]+/).some(w => w.startsWith(q))) s = 3;
      else if (name.includes(q)) s = 2;
      else if (extra.includes(q)) s = 1;
      if (s > 0)
        scored.push({ e, s });
    }
    scored.sort((a, b) => b.s - a.s || a.e.name.localeCompare(b.e.name));
    return scored.slice(0, 7).map(x => x.e);
  }
  readonly property int count: results.length + (query !== "" ? 1 : 0)
  onQueryChanged: selected = 0

  function activate(i) {
    if (i < results.length) {
      results[i].execute();
    } else if (query !== "") {
      Friday.ask(input.text.trim());
    }
    root.done();
  }

  ColumnLayout {
    id: column
    width: parent.width
    spacing: 10

    Rectangle {
      Layout.fillWidth: true
      implicitHeight: 44
      radius: height / 2
      color: Theme.surface
      border.width: 1
      border.color: Theme.alpha(Theme.accent, 0.5)

      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 10
        Glyph { text: Icons.search; size: 18; color: Theme.accent }
        TextInput {
          id: input
          Layout.fillWidth: true
          focus: true
          color: Theme.text
          selectionColor: Theme.alpha(Theme.accent, 0.4)
          font.family: Theme.sans
          font.pixelSize: Theme.fontM + 1
          clip: true
          onAccepted: root.activate(root.selected)
          Keys.onDownPressed: root.selected = Math.min(root.count - 1, root.selected + 1)
          Keys.onUpPressed: root.selected = Math.max(0, root.selected - 1)
          Keys.onTabPressed: root.selected = (root.selected + 1) % Math.max(1, root.count)

          Label {
            anchors.verticalCenter: parent.verticalCenter
            visible: input.text === ""
            text: "Search apps, or ask FRIDAY"
            color: Theme.faint
            font.pixelSize: Theme.fontM + 1
          }
        }
      }
    }

    Repeater {
      model: root.results
      Rectangle {
        id: row
        required property var modelData
        required property int index
        Layout.fillWidth: true
        implicitHeight: 46
        radius: Theme.radiusS + 2
        color: index === root.selected ? Theme.raised : mouse.containsMouse ? Theme.surface : "transparent"

        Rectangle {
          visible: row.index === root.selected
          width: 3
          height: 20
          radius: 2
          anchors.left: parent.left
          anchors.verticalCenter: parent.verticalCenter
          color: Theme.accent
        }

        RowLayout {
          anchors.fill: parent
          anchors.leftMargin: 14
          anchors.rightMargin: 14
          spacing: 12
          Item {
            implicitWidth: 26
            implicitHeight: 26
            readonly property bool isFriday: row.modelData.id === "friday" || row.modelData.name === "FRIDAY"
            readonly property string iconSource: Quickshell.iconPath(row.modelData.icon, true)
            Orb { anchors.centerIn: parent; size: 22; visible: parent.isFriday }
            IconImage {
              anchors.fill: parent
              visible: !parent.isFriday && parent.iconSource !== ""
              source: parent.iconSource
              asynchronous: true
            }
            Glyph {
              anchors.centerIn: parent
              visible: !parent.isFriday && parent.iconSource === ""
              text: Icons.app
              size: 22
              color: Theme.subtext
            }
          }
          ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Label { Layout.fillWidth: true; text: row.modelData.name; font.weight: Font.Medium }
            Label {
              Layout.fillWidth: true
              visible: text !== ""
              text: row.modelData.genericName || row.modelData.comment || ""
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
          onClicked: root.activate(row.index)
        }
      }
    }

    // Hand it to FRIDAY.
    Rectangle {
      Layout.fillWidth: true
      visible: root.query !== ""
      readonly property bool isSel: root.selected === root.results.length
      implicitHeight: 46
      radius: Theme.radiusS + 2
      color: isSel ? Theme.alpha(Theme.accent, 0.16) : askMouse.containsMouse ? Theme.surface : "transparent"
      border.width: isSel ? 1 : 0
      border.color: Theme.alpha(Theme.accent, 0.5)

      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 14
        anchors.rightMargin: 14
        spacing: 12
        Orb { size: 22 }
        Label {
          Layout.fillWidth: true
          text: "Ask FRIDAY: “" + input.text.trim() + "”"
          color: Theme.accent
          font.weight: Font.Medium
        }
      }
      MouseArea {
        id: askMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.activate(root.results.length)
      }
    }

    Label {
      visible: root.query !== "" && root.results.length === 0
      text: "No apps match."
      color: Theme.faint
      font.pixelSize: Theme.fontS + 1
    }
  }
}
