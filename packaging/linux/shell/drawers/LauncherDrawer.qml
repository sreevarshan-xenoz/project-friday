import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Widgets
import qs.common
import qs.components
import qs.services

// Find an app, one of FRIDAY's quick commands or a file, or hand the words to
// FRIDAY. Type to filter, arrows to move, Enter to launch. The last row is
// always "Ask FRIDAY". Typing here sends nothing: file names are looked up on
// this computer (with fd, when it is installed), and only the last row asks.
FocusScope {
  id: root

  signal done()

  implicitWidth: 420
  implicitHeight: column.implicitHeight

  property int selected: 0
  readonly property string query: input.text.trim().toLowerCase()

  // Rows are plain objects: { kind: "app" | "cmd" | "file", name, sub, ... }.
  function appRow(e) {
    return { kind: "app", name: e.name, sub: e.genericName || e.comment || "", icon: e.icon, id: e.id, entry: e };
  }

  readonly property var results: {
    const q = query;
    const apps = DesktopEntries.applications.values.filter(e => !e.noDisplay);
    if (q === "")
      return apps.slice().sort((a, b) => a.name.localeCompare(b.name)).slice(0, 7).map(appRow);
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
    const rows = scored.slice(0, 5).map(x => appRow(x.e));
    for (const c of Commands.match(q).slice(0, 3))
      rows.push({ kind: "cmd", name: c.name, sub: c.hint, run: c.run });
    for (const f of fileHits)
      rows.push({ kind: "file", name: f.name, sub: f.dir, path: f.path });
    return rows;
  }

  // Files whose names contain the query, newest lookup only.
  property var fileHits: []
  property string fileQuery: ""
  readonly property string home: Quickshell.env("HOME") || ""

  Timer {
    id: debounce
    interval: 250
    onTriggered: {
      if (finder.running) finder.running = false;
      root.fileQuery = root.query;
      finder.command = ["fd", "--max-results", "4", "--type", "f", "--ignore-case", "--fixed-strings", "--", root.fileQuery, root.home];
      finder.running = true;
    }
  }
  Process {
    id: finder
    stdout: StdioCollector {
      onStreamFinished: {
        if (root.fileQuery !== root.query) return;
        root.fileHits = text.split("\n").filter(p => p !== "").map(p => {
          const cut = p.lastIndexOf("/");
          return { path: p, name: p.slice(cut + 1), dir: p.slice(0, cut).replace(root.home, "~") };
        });
      }
    }
  }
  readonly property int count: results.length + (query !== "" ? 1 : 0)
  onQueryChanged: {
    selected = 0;
    fileHits = [];
    if (query.length >= 3) debounce.restart(); else debounce.stop();
  }

  function activate(i) {
    if (i < results.length) {
      const r = results[i];
      if (r.kind === "app") r.entry.execute();
      else if (r.kind === "cmd") Quickshell.execDetached(r.run);
      else Quickshell.execDetached(["xdg-open", r.path]);
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
      color: Theme.glassWell
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
            text: "Search apps and files, or ask FRIDAY"
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
            readonly property bool isApp: row.modelData.kind === "app"
            readonly property bool isFriday: row.modelData.kind === "cmd" || row.modelData.id === "friday" || row.modelData.name === "FRIDAY"
            readonly property string iconSource: isApp ? AppIcons.source(row.modelData.icon) : ""
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
              text: parent.isApp ? Icons.app : Icons.file
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
              text: row.modelData.sub
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
      text: "Nothing matches."
      color: Theme.faint
      font.pixelSize: Theme.fontS + 1
    }
  }
}
