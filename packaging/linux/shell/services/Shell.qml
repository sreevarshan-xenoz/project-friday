pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland

// Which drawer is open, and on which screen. At most one drawer is open at a
// time. Drawers open on the focused monitor (Hyprland) or the first screen.
//
// IPC (`qs -p /usr/share/friday/shell ipc call shell <fn>`, or `friday shell <fn>`):
//   toggle <drawer>   friday | control | calendar | launcher | session
//   open <drawer>, close
Singleton {
  id: root

  readonly property var drawers: ["friday", "control", "calendar", "launcher", "session"]

  property string drawer: ""
  property string screen: ""

  function focusedScreen() {
    const m = Hyprland.focusedMonitor;
    if (m && m.name)
      return m.name;
    return Quickshell.screens.length > 0 ? Quickshell.screens[0].name : "";
  }

  function open(name, screenName) {
    if (drawers.indexOf(name) === -1)
      return;
    root.screen = screenName || focusedScreen();
    root.drawer = name;
  }
  function close() { root.drawer = ""; }
  // A tray menu is open (it keeps an auto-hiding bar up).
  property bool menuOpen: false
  function toggle(name, screenName) {
    const s = screenName || focusedScreen();
    if (root.drawer === name && root.screen === s)
      close();
    else
      open(name, s);
  }
  function isOpen(name, screenName) { return root.drawer === name && root.screen === screenName; }

  IpcHandler {
    target: "shell"
    function toggle(drawer: string): void { root.toggle(drawer); }
    function open(drawer: string): void { root.open(drawer); }
    function close(): void { root.close(); }
  }
}
