pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.common

// FRIDAY's Neon sound set in FRIDAY Shell (friday sounds neon; switch:
// shell-sounds): a soft tone for a new notification (not FRIDAY's own: its
// answers have their cue), a short rise when a drawer opens and a fall when
// it closes. pw-play plays each one and exits; nothing stays running.
Singleton {
  id: root

  property string set: ""
  readonly property bool on: set === "neon" && Config.featureOn("shell-sounds")
  readonly property string dir: Quickshell.env("FRIDAY_NEON_SOUNDS_DIR") || "/usr/share/friday/sounds/neon"

  function play(name) {
    if (root.on)
      Quickshell.execDetached(["pw-play", "--volume", "0.5", root.dir + "/" + name + ".oga"]);
  }

  // The flag `friday sounds` writes: missing = off, "neon" = this set.
  FileView {
    id: flag
    path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config") + "/friday/sounds"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.set = text().trim()
    onLoadFailed: root.set = ""
  }
  // A watch on a file that doesn't exist yet never fires when it appears.
  Timer { interval: 2000; running: true; repeat: true; onTriggered: flag.reload() }

  // Opening from nothing and closing to nothing; going from one drawer
  // straight to another stays quiet.
  property string lastDrawer: ""
  Connections {
    target: Shell
    function onDrawerChanged() {
      if (root.lastDrawer === "" && Shell.drawer !== "") root.play("open");
      else if (root.lastDrawer !== "" && Shell.drawer === "") root.play("close");
      root.lastDrawer = Shell.drawer;
    }
  }
}
