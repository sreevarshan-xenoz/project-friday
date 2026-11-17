pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// User settings from ~/.config/friday/shell.json. Every key is optional and
// the file itself may be missing or malformed: each accessor falls back to a
// default instead of erroring. Edits apply live.
//
//   {
//     "style": "ember",         // "ember" (bar flush with the top edge),
//                               // "island" (three floating pills) or "dock"
//                               // (a floating bar at the bottom)
//     "palette": "ember",       // FRIDAY's own: "ember", "frost", "moss",
//                               // "dawn" (light); or "classic" (follow the
//                               // desktop theme). "friday" = "ember"; the old
//                               // name "omarchy" = "classic".
//     "clock24h": true,
//     "workspaces": 5,          // workspace slots always shown in the bar
//     "notificationTimeout": 6  // seconds; FRIDAY's own answers stay longer
//   }
Singleton {
  id: root

  readonly property string path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config")
    + "/friday/shell.json"

  property var settings: ({})

  readonly property var styles: ["ember", "island", "dock"]
  readonly property var palettes: ["ember", "frost", "moss", "dawn", "classic"]
  readonly property string style: styles.includes(pick("style", "ember")) ? pick("style", "ember") : "ember"
  readonly property string palette: pick("palette", "ember") === "friday" ? "ember" : pick("palette", "ember") === "omarchy" ? "classic" : pick("palette", "ember")
  readonly property bool clock24h: pick("clock24h", true) === true
  readonly property int workspaces: Math.max(1, Math.min(10, Number(pick("workspaces", 5)) || 5))
  readonly property int notificationTimeout: Math.max(2, Number(pick("notificationTimeout", 6)) || 6)

  function pick(key, fallback) {
    const v = root.settings[key];
    return (v === undefined || v === null) ? fallback : v;
  }

  // Change one setting (the Look picker): merged into shell.json, which
  // every accessor re-reads, so it applies live.
  function set(key, value) {
    const next = Object.assign({}, root.settings);
    next[key] = value;
    root.settings = next;
    file.setText(JSON.stringify(next, null, 2) + "\n");
  }

  FileView {
    id: file
    path: root.path
    watchChanges: true
    atomicWrites: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try {
        const obj = JSON.parse(text());
        root.settings = (obj && typeof obj === "object") ? obj : {};
      } catch (e) {
        console.warn("friday shell: ignoring malformed " + root.path);
        root.settings = {};
      }
    }
    onLoadFailed: root.settings = {}
  }

  // A watch on a file that doesn't exist yet never fires when it appears, and
  // an atomic replace (friday shell style …) can drop it: re-read now and then.
  Timer {
    interval: 2000
    running: true
    repeat: true
    onTriggered: file.reload()
  }
}
