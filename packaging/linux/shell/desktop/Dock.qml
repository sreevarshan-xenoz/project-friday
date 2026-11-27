import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The dock (shell.json "dock": true; Look > Dock, or `friday shell dock on`;
// off unless turned on): your pinned apps ("dockApps", desktop entry ids)
// and the ones running, along the bottom's middle. It stays out of the way:
// the pointer at the bottom edge brings it up, and it slides away once the
// pointer leaves. A click raises a running app or starts a pinned one; the
// dot under an icon means it's running, the longer one that it's in front.
// An idea from end-4/dots-hyprland's dock (see CREDITS.md).
PanelWindow {
  id: dock

  required property ShellScreen modelData
  screen: modelData

  readonly property bool shown: edge.hovered || body.hovered
  // The app under the pointer, named above the dock.
  property string hoverName: ""
  property real reveal: shown ? 1 : 0
  Behavior on reveal { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }

  // Pinned entries first (in their order), then running apps not pinned.
  readonly property var pinned: (Config.pick("dockApps", []) || []).map(id => DesktopEntries.byId(String(id))).filter(e => e)
  readonly property var items: {
    const out = pinned.map(e => ({ entry: e, id: e.id }));
    for (const t of ToplevelManager.toplevels.values) {
      const e = DesktopEntries.heuristicLookup(t.appId);
      const id = e ? e.id : t.appId;
      if (id && !out.some(o => o.id === id))
        out.push({ entry: e, id: id, appId: t.appId });
    }
    return out;
  }
  function windowsOf(item) {
    const want = String(item.appId || item.id).toLowerCase();
    return ToplevelManager.toplevels.values.filter(t => {
      const a = String(t.appId).toLowerCase();
      return a === want || (item.entry && DesktopEntries.heuristicLookup(t.appId) === item.entry);
    });
  }

  anchors { bottom: true; left: true; right: true }
  implicitHeight: 76
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "friday-dock"
  WlrLayershell.layer: WlrLayer.Top
  // Only the edge strip, and the dock while it's up, take the pointer.
  mask: Region {
    item: edgeStrip
    Region { item: dock.reveal > 0.01 ? body : null }
  }

  Item {
    id: edgeStrip
    anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
    width: Math.max(body.width, 240)
    height: 3
    HoverHandler { id: edge }
  }

  Label {
    visible: dock.reveal > 0.5 && body.hovered && dock.hoverName !== ""
    anchors { horizontalCenter: body.horizontalCenter; bottom: body.top; bottomMargin: 6 }
    text: dock.hoverName
    font.pixelSize: Theme.fontS + 1
    color: Theme.text
  }

  Rectangle {
    id: body
    readonly property alias hovered: bodyHover.hovered
    anchors.horizontalCenter: parent.horizontalCenter
    y: parent.height - 8 - (height + 8) * dock.reveal + (1 - dock.reveal) * 8
    width: row.implicitWidth + 20
    height: 58
    radius: Math.min(18, Theme.radius)
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
    opacity: dock.reveal
    visible: dock.reveal > 0.01
    HoverHandler { id: bodyHover }

    RowLayout {
      id: row
      anchors.centerIn: parent
      spacing: 6
      Repeater {
        model: dock.items
        Item {
          id: slot
          required property var modelData
          readonly property var wins: dock.windowsOf(modelData)
          readonly property bool front: wins.some(w => w.activated)
          implicitWidth: 44
          implicitHeight: 48

          Rectangle {
            anchors.centerIn: icon
            width: 42; height: 42
            radius: 12
            color: tap.hovered ? Theme.raised : "transparent"
          }
          Image {
            id: icon
            anchors.horizontalCenter: parent.horizontalCenter
            y: 2 - (tap.hovered && Theme.motion ? 3 : 0)
            Behavior on y { NumberAnimation { duration: Theme.fast; easing.type: Theme.easing } }
            width: 34; height: 34
            sourceSize: Qt.size(64, 64)
            source: AppIcons.source(slot.modelData.entry ? slot.modelData.entry.icon : slot.modelData.appId)
            asynchronous: true
          }
          Glyph {
            visible: icon.status !== Image.Ready
            anchors.centerIn: icon
            text: Icons.apps
            size: 22
            color: Theme.faint
          }
          // Running: a dot; in front: a longer one in the accent.
          Rectangle {
            visible: slot.wins.length > 0
            anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom }
            width: slot.front ? 14 : 5
            height: 4
            radius: 2
            color: slot.front ? Theme.accent : Theme.subtext
            Behavior on width { NumberAnimation { duration: Theme.fast } }
          }
          HoverHandler { id: tap }
          TapHandler {
            onTapped: {
              if (slot.wins.length > 0) {
                // In front already: the next of its windows; otherwise its first.
                const i = slot.wins.findIndex(w => w.activated);
                slot.wins[(i + 1) % slot.wins.length].activate();
              } else if (slot.modelData.entry) {
                slot.modelData.entry.execute();
              }
            }
          }
          Binding {
            when: tap.hovered
            target: dock
            property: "hoverName"
            value: slot.modelData.entry ? slot.modelData.entry.name : slot.modelData.id
          }
        }
      }
    }
  }
}
