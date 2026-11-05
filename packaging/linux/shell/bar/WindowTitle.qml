import QtQuick
import Quickshell.Wayland
import qs.common
import qs.components

// The focused window's title, quiet and truncated.
Label {
  readonly property var win: ToplevelManager.activeToplevel
  text: win && win.activated ? (win.title || win.appId || "") : ""
  color: Theme.subtext
  font.pixelSize: Theme.fontS + 1
  maximumLineCount: 1
}
