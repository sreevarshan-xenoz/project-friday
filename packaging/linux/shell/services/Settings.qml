pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY Settings: the app window for everything the `friday` commands do,
// for people who don't use a terminal. `friday app` opens it.
Singleton {
  id: root

  property bool open: false
  property string page: "Home"

  function show(page) {
    if (page) root.page = page;
    root.open = true;
  }

  IpcHandler {
    target: "settings"
    function open(page: string): void { root.show(page); }
    function close(): void { root.open = false; }
    function page(): string { return root.open ? root.page : ""; }
  }
}
