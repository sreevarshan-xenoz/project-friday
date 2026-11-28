import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The wallpaper picker (Super+Ctrl+T, `friday shell wallpaper`): FRIDAY's
// wallpapers on an arc, the middle one large and tilting toward you, the
// rest leaning away; the middle one shows softly behind it all. Arrows,
// the wheel or a drag move along; a click on a side one brings it round;
// Enter or a click on the middle one sets it (`friday wallpaper`, which
// also works out its accent). Pictures are decoded small and only for the
// few on the arc.
PanelWindow {
  id: picker

  required property ShellScreen modelData
  screen: modelData

  property bool open: false
  visible: open || fade.running
  // Still showing: open, or fading out (the pictures stay until it's gone).
  readonly property bool showing: open || stage.opacity > 0
  property var all: []
  property string folder: ""
  readonly property var shown: folder ? all.filter(p => p.split("/").slice(-2, -1)[0] === folder) : all
  readonly property var folders: {
    const seen = [];
    for (const p of all) { const f = p.split("/").slice(-2, -1)[0]; if (f && !seen.includes(f)) seen.push(f); }
    return seen;
  }
  readonly property string current: Config.pick("wallpaper", "")
  readonly property string centred: arc.count > 0 && arc.currentIndex >= 0 ? shown[arc.currentIndex] || "" : ""

  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  // The dark ground is the window's own clear colour (free), not one more
  // full-screen layer to blend every frame.
  color: Theme.alpha(Theme.bg, stage.opacity)
  WlrLayershell.namespace: "friday-wallpaper"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

  function toggle() {
    if (!open) list.running = true;
    open = !open;
  }
  function apply(path) {
    if (!path) return;
    Quickshell.execDetached(["friday", "wallpaper", path]);
    open = false;
  }
  function label(path) {
    const n = path.split("/").pop().replace(/\.[a-z]+$/i, "").replace(/-\d+$/, "").replace(/^\d+-/, "").replace(/-/g, " ");
    return n.charAt(0).toUpperCase() + n.slice(1);
  }

  Process {
    id: list
    command: ["friday", "wallpaper", "list"]
    stdout: StdioCollector {
      onStreamFinished: {
        picker.all = text.split("\n").filter(l => l !== "");
        const i = picker.shown.indexOf(picker.current);
        arc.currentIndex = Math.max(0, i);
      }
    }
  }

  IpcHandler {
    target: "wallpaper"
    function toggle(): void { if (Shell.focusedScreen() === picker.modelData.name) picker.toggle(); }
    function close(): void { picker.open = false; }
  }

  Shortcut { sequence: "Escape"; enabled: picker.open; onActivated: picker.open = false }
  Shortcut { sequence: "Left"; enabled: picker.open; onActivated: arc.decrementCurrentIndex() }
  Shortcut { sequence: "Right"; enabled: picker.open; onActivated: arc.incrementCurrentIndex() }
  Shortcut { sequence: "Return"; enabled: picker.open; onActivated: picker.apply(picker.centred) }

  // The dimmed desktop with the middle wallpaper drifting in behind; a
  // click on it closes. The backdrop waits for the arc to settle before it
  // decodes, so a fast spin doesn't decode every picture it passes.
  Item {
    id: stage
    anchors.fill: parent
    opacity: picker.open ? 1 : 0
    Behavior on opacity { NumberAnimation { id: fade; duration: Theme.medium; easing.type: Theme.easing } }

    Image {
      id: backdrop
      anchors.fill: parent
      fillMode: Image.PreserveAspectCrop
      sourceSize: Qt.size(640, 360)
      asynchronous: true
      opacity: status === Image.Ready ? 0.35 : 0
      Behavior on opacity { NumberAnimation { duration: Theme.slow; easing.type: Theme.easing } }
      Timer {
        id: settle
        interval: 220
        onTriggered: backdrop.source = picker.centred ? "file://" + picker.centred : ""
      }
      Connections {
        target: picker
        function onCentredChanged() { settle.restart(); }
      }
    }
    Rectangle {
      anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
      height: parent.height * 0.5
      gradient: Gradient {
        GradientStop { position: 0; color: "transparent" }
        GradientStop { position: 1; color: Theme.alpha(Theme.bg, 0.9) }
      }
    }
    MouseArea { anchors.fill: parent; onClicked: picker.open = false }

    // Folders: every picture, or one set.
    Row {
      z: 2
      anchors.horizontalCenter: parent.horizontalCenter
      y: parent.height * 0.12
      spacing: 8
      Repeater {
        model: [""].concat(picker.folders)
        Chip {
          required property string modelData
          text: modelData ? modelData.charAt(0).toUpperCase() + modelData.slice(1) : "All"
          primary: picker.folder === modelData
          onClicked: { picker.folder = modelData; arc.currentIndex = 0; }
        }
      }
    }

    // Only the arc's band takes drags, so a click above or below it closes.
    PathView {
      id: arc
      x: 0
      y: parent.height * 0.3
      width: parent.width
      height: parent.height * 0.45
      model: picker.showing ? picker.shown : []
      pathItemCount: Math.min(7, count)
      cacheItemCount: 2
      preferredHighlightBegin: 0.5
      preferredHighlightEnd: 0.5
      highlightRangeMode: PathView.StrictlyEnforceRange
      highlightMoveDuration: Theme.motion ? 320 : 0
      snapMode: PathView.SnapToItem
      focus: true

      // A shallow arc that rises to the middle.
      path: Path {
        startX: -arc.width * 0.05; startY: arc.height * 0.71
        PathAttribute { name: "k"; value: 0 }
        PathQuad { x: arc.width * 0.5; y: arc.height * 0.38; controlX: arc.width * 0.25; controlY: arc.height * 0.4 }
        PathAttribute { name: "k"; value: 1 }
        PathQuad { x: arc.width * 1.05; y: arc.height * 0.71; controlX: arc.width * 0.75; controlY: arc.height * 0.4 }
        PathAttribute { name: "k"; value: 0 }
      }

      // Scrolling steps along the arc. (A button-less MouseArea over the
      // pictures: a WheelHandler here never got the wheel events.)
      MouseArea {
        anchors.fill: parent
        z: 100
        acceptedButtons: Qt.NoButton
        onWheel: e => {
          const d = e.angleDelta.y !== 0 ? e.angleDelta.y : e.angleDelta.x;
          if (d < 0) arc.incrementCurrentIndex();
          else if (d > 0) arc.decrementCurrentIndex();
        }
      }

      delegate: Item {
        id: slot
        required property string modelData
        required property int index
        readonly property real k: PathView.k ?? 0
        readonly property bool middle: PathView.isCurrentItem
        // Which side of the middle it sits, for the lean.
        readonly property real side: x + width / 2 < arc.width / 2 ? 1 : -1

        width: 360
        height: 203
        z: k * 10
        scale: 0.55 + 0.6 * k
        opacity: 0.25 + 0.75 * k

        transform: Rotation {
          origin.x: slot.width / 2
          origin.y: slot.height / 2
          axis { x: 0; y: 1; z: 0 }
          angle: slot.middle ? 0 : slot.side * 38 * (1 - slot.k)
        }

        Rectangle {
          anchors.fill: parent
          anchors.margins: -3
          radius: 17
          color: "transparent"
          border.width: slot.middle ? 2 : 0
          border.color: Theme.accent
          visible: slot.middle
        }
        Rectangle {
          id: card
          anchors.fill: parent
          radius: 14
          color: Theme.surface
          clip: true
          Image {
            anchors.fill: parent
            source: "file://" + slot.modelData
            sourceSize: Qt.size(384, 216)
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.medium } }
          }
          // The one on the desktop now.
          Rectangle {
            visible: slot.modelData === picker.current
            anchors { top: parent.top; right: parent.right; margins: 10 }
            width: nowLabel.implicitWidth + 16
            height: 22
            radius: 11
            color: Theme.alpha(Theme.bg, 0.75)
            Label { id: nowLabel; anchors.centerIn: parent; text: "Now"; font.pixelSize: Theme.fontS }
          }
        }

        TapHandler {
          onTapped: slot.middle ? picker.apply(slot.modelData) : arc.currentIndex = slot.index
        }
      }
    }

    ColumnLayout {
      anchors.horizontalCenter: parent.horizontalCenter
      y: parent.height * 0.74
      spacing: 6
      Label {
        Layout.alignment: Qt.AlignHCenter
        text: picker.centred ? picker.label(picker.centred) : (picker.all.length ? "" : "No wallpapers found")
        font.pixelSize: Theme.fontL
        font.weight: Font.DemiBold
      }
      Label {
        Layout.alignment: Qt.AlignHCenter
        text: arc.count ? (arc.currentIndex + 1) + " of " + arc.count + "  ·  ← → to look  ·  Enter to set  ·  Esc to close" : ""
        color: Theme.subtext
        font.pixelSize: Theme.fontS
      }
    }
  }
}
