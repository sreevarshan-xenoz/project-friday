import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common

// When the wallpaper changes, the old one fades out over the new instead of
// switching at once (switch: wallpaper-fade; none with motion off). swaybg
// draws the wallpaper; this window exists only for the fade, so the picture
// it holds is let go when the fade ends. An idea from liixini/skwd-wall's
// transitions (see CREDITS.md).
Scope {
  id: root

  required property ShellScreen modelData
  readonly property string current: String(Config.pick("wallpaper", ""))
  property string last: ""
  property string from: ""

  onCurrentChanged: {
    if (last !== "" && current !== "" && current !== last
        && Config.featureOn("wallpaper-fade") && Theme.motion) {
      from = last;
      fade.active = true;
    }
    last = current;
  }
  Component.onCompleted: last = current

  LazyLoader {
    id: fade
    PanelWindow {
      screen: root.modelData
      anchors { top: true; bottom: true; left: true; right: true }
      exclusionMode: ExclusionMode.Ignore
      color: "transparent"
      // Above swaybg's new surface, under the windows (desktop widgets come
      // back with the new picture). Not "friday-…": the frosted-glass blur
      // rule matches those, and would blur the new picture through it.
      WlrLayershell.namespace: "friday.wallpaper-fade"
      WlrLayershell.layer: WlrLayer.Bottom
      mask: Region {}

      Image {
        id: old
        anchors.fill: parent
        source: "file://" + root.from.split("/").map(encodeURIComponent).join("/")
        fillMode: Image.PreserveAspectCrop
        sourceSize: Qt.size(root.modelData.width, root.modelData.height)
        asynchronous: false
        cache: false
        // Held while the new swaybg starts (about 0.3 s), then faded.
        SequentialAnimation on opacity {
          running: old.status === Image.Ready
          PauseAnimation { duration: 400 }
          NumberAnimation { from: 1; to: 0; duration: 700; easing.type: Easing.InOutQuad }
          ScriptAction { script: fade.active = false }
        }
      }
      // A picture that can't be read ends it at once.
      Timer { running: old.status === Image.Error; interval: 1; onTriggered: fade.active = false }
    }
  }
}
