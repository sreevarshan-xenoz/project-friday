import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The shelf (shell.json "shelf": true; Look > Shelf; off unless turned on):
// a panel on the right edge that slides out when the pointer reaches it.
// Drop files or text on it to keep them for later; it also lists the last
// few things you copied (when friday clips is on). A click copies a text, or
// opens a file; a kept file can be dragged out again into any app. Kept
// things live in ~/.local/state/friday/shelf.json, on this computer only.
// An idea from Deepender25/Edge-Drop (see CREDITS.md).
PanelWindow {
  id: shelf

  required property ShellScreen modelData
  screen: modelData

  readonly property string stateDir: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state") + "/friday"
  property var kept: []
  property var clips: []
  // Up while the pointer is on the edge, the panel or the gap between them,
  // while a drag is over either, or while a kept thing is dragged out.
  readonly property bool shown: edge.hovered || zoneHover.hovered || drop.containsDrag || edgeDrop.containsDrag || dragging
  property bool dragging: false
  property real reveal: shown ? 1 : 0
  Behavior on reveal { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

  anchors { top: true; bottom: true; right: true }
  margins.top: Theme.barReach + Theme.gap
  margins.bottom: Theme.gap
  implicitWidth: 296
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-shelf"
  WlrLayershell.layer: WlrLayer.Top
  mask: Region {
    item: edgeStrip
    Region { item: shelf.reveal > 0.01 ? zone : null }
  }

  function save() {
    keptFile.setText(JSON.stringify(shelf.kept.slice(0, 30)) + "\n");
  }
  function keep(kind, value) {
    if (!value) return;
    shelf.kept = [{ kind: kind, value: value }].concat(shelf.kept.filter(k => !(k.kind === kind && k.value === value)));
    shelf.save();
  }
  function drop(i) {
    shelf.kept = shelf.kept.filter((_, j) => j !== i);
    shelf.save();
  }
  // A path as a file URL, each part encoded (so "#", "?" and spaces survive).
  function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
  }
  function take(d) {
    if (d.hasUrls) {
      for (const u of d.urls) {
        const s = String(u);
        if (s.startsWith("file://")) shelf.keep("file", decodeURIComponent(s.slice(7)));
      }
    } else if (d.hasText) {
      shelf.keep("text", d.text);
    }
    d.acceptProposedAction();
  }
  function use(item) {
    if (item.kind === "file")
      Qt.openUrlExternally(shelf.fileUrl(item.value));
    else
      Quickshell.execDetached(["wl-copy", "--", item.value]);
  }
  function short(item) {
    return item.kind === "file" ? item.value.split("/").pop() : item.value.replace(/\s+/g, " ").slice(0, 70);
  }

  FileView {
    id: keptFile
    path: shelf.stateDir + "/shelf.json"
    printErrors: false
    atomicWrites: true
    onLoaded: { try { const a = JSON.parse(text()); shelf.kept = Array.isArray(a) ? a : []; } catch (e) { shelf.kept = []; } }
  }
  // The newest clips, read when the shelf comes out.
  FileView {
    id: clipsFile
    path: shelf.stateDir + "/clips.jsonl"
    printErrors: false
    onLoaded: {
      const out = [];
      for (const line of text().split("\n").reverse()) {
        if (out.length >= 6) break;
        try { const c = JSON.parse(line); if (c && c.text) out.push({ kind: "text", value: c.text }); } catch (e) {}
      }
      shelf.clips = out;
    }
    onLoadFailed: shelf.clips = []
  }
  onShownChanged: if (shown) clipsFile.reload()

  Item {
    id: edgeStrip
    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
    width: 3
    HoverHandler { id: edge }
    // A file dragged to the edge brings the shelf out.
    DropArea { id: edgeDrop; anchors.fill: parent; onDropped: d => shelf.take(d) }
  }

  // The panel and the gap from it to the edge: hovering or dragging over any
  // of it keeps the shelf out.
  Item {
    id: zone
    x: parent.width - (panel.width + Theme.gap) * shelf.reveal
    width: parent.width - x
    height: parent.height
    HoverHandler { id: zoneHover }
    DropArea { id: drop; anchors.fill: parent; onDropped: d => shelf.take(d) }

    Rectangle {
      id: panel
      width: shelf.width - Theme.gap
      height: parent.height
      visible: shelf.reveal > 0.01
      opacity: shelf.reveal
      radius: Theme.radius
      color: Theme.glass
      border.width: 1
      border.color: drop.containsDrag ? Theme.accent : Theme.glassEdge

      ColumnLayout {
        anchors { fill: parent; margins: Theme.pad }
        spacing: 8

        Label { text: "Shelf"; font.pixelSize: Theme.fontL; font.weight: Font.DemiBold }
        Label {
          Layout.fillWidth: true
          visible: shelf.kept.length === 0
          text: "Drop files or text here to keep them for later."
          wrapMode: Text.WordWrap
          color: Theme.faint
          font.pixelSize: Theme.fontS
        }

        component Entry: Rectangle {
          id: entry
          required property var item
          property bool removable: false
          signal removed()
          Layout.fillWidth: true
          implicitHeight: 34
          radius: Theme.radiusS
          color: hover.hovered ? Theme.raised : Theme.alpha(Theme.text, 0.04)
          HoverHandler { id: hover }
          // A kept file can be dragged out into another app.
          Drag.active: dragger.active
          Drag.dragType: Drag.Automatic
          Drag.supportedActions: Qt.CopyAction
          Drag.mimeData: entry.item.kind === "file" ? { "text/uri-list": shelf.fileUrl(entry.item.value) } : { "text/plain": entry.item.value }
          DragHandler { id: dragger; target: null }
          Binding { when: dragger.active; target: shelf; property: "dragging"; value: true }
          RowLayout {
            anchors { fill: parent; leftMargin: 10; rightMargin: 6 }
            spacing: 8
            Glyph { text: entry.item.kind === "file" ? Icons.file : Icons.copy; size: 14; color: Theme.subtext }
            Label {
              Layout.fillWidth: true
              text: shelf.short(entry.item)
              elide: Text.ElideRight
              font.pixelSize: Theme.fontS + 1
            }
            IconButton {
              id: remove
              visible: entry.removable && hover.hovered
              icon: Icons.close
              size: 22
              iconSize: 12
              tooltip: "Remove"
              onClicked: entry.removed()
            }
          }
          // Not when the tap was on the remove button.
          TapHandler {
            onTapped: p => {
              if (remove.visible && remove.contains(remove.mapFromItem(entry, p.position))) return;
              shelf.use(entry.item);
            }
          }
        }

        Repeater {
          model: shelf.kept
          Entry {
            required property var modelData
            required property int index
            item: modelData
            removable: true
            onRemoved: shelf.drop(index)
          }
        }

        Label {
          Layout.topMargin: 6
          visible: shelf.clips.length > 0
          text: "Copied lately"
          font.pixelSize: Theme.fontS
          color: Theme.faint
        }
        Repeater {
          model: shelf.clips
          Entry {
            required property var modelData
            item: modelData
          }
        }
        Item { Layout.fillHeight: true }
      }
    }
  }
}
