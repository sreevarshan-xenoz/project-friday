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
//                               // "lean" (the same bar; thin borders, small
//                               // gaps, no blur), "island" (three floating
//                               // pills), "dock" (a floating bar at the bottom)
//                               // "rail" (a slim column down the left edge)
//                               // or "keystone" (one notch in the top's middle)
//     "palette": "core",        // FRIDAY's own: "core" (the logo's obsidian
//                               // and red), "ember", "polar", "frost", "moss",
//                               // "dawn" (light); or "classic" (follow the
//                               // desktop theme). "friday" = "ember"; the old
//                               // name "omarchy" = "classic".
//     "clock24h": true,
//     "workspaces": 5,          // workspace slots always shown in the bar
//     "notificationTimeout": 6, // seconds; FRIDAY's own answers stay longer
//     "wallpaper": "/usr/share/friday/wallpapers/core/...jpg",
//                               // the desktop wallpaper, which `friday wallpaper`
//                               // sets and swaybg draws (FRIDAY OS); unset = your
//                               // own tool does; the lock screen shows it too
//     "polkit": true            // FRIDAY Shell asks for administrator passwords
//                               // (FRIDAY OS); off = your desktop's agent does
//   }
Singleton {
  id: root

  readonly property string path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config")
    + "/friday/shell.json"

  property var settings: ({})

  // Features switched off (`friday feature <name> off`): one name per line.
  property var featuresOff: []
  function featureOn(name) { return !root.featuresOff.includes(name); }

  readonly property var styles: ["ember", "lean", "island", "dock", "rail", "keystone"]
  readonly property var palettes: ["core", "ember", "polar", "frost", "moss", "dawn", "contrast", "classic"]
  readonly property string style: styles.includes(pick("style", "ember")) ? pick("style", "ember") : "ember"
  readonly property string palette: pick("palette", "core") === "friday" ? "ember" : pick("palette", "core") === "omarchy" ? "classic" : pick("palette", "core")
  readonly property bool clock24h: pick("clock24h", true) === true
  readonly property int workspaces: Math.max(1, Math.min(10, Number(pick("workspaces", 5)) || 5))
  readonly property int notificationTimeout: Math.max(2, Number(pick("notificationTimeout", 6)) || 6)
  readonly property bool polkit: pick("polkit", false) === true
  // The workspaces show their apps' icons (off unless turned on).
  readonly property bool workspaceIcons: pick("workspaceIcons", false) === true

  function pick(key, fallback) {
    const v = root.settings[key];
    return (v === undefined || v === null) ? fallback : v;
  }

  // Change one setting (the Look picker): merged into shell.json, which
  // every accessor re-reads, so it applies live.
  function set(key, value) {
    const next = Object.assign({}, root.settings);
    next[key] = value;
    // A chosen palette brings its own accent, not the wallpaper's.
    if (key === "palette") delete next.wallpaperAccent;
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

  FileView {
    id: offFile
    path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config") + "/friday/features-off"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.featuresOff = text().split("\n").map(l => l.trim()).filter(l => l !== "")
    onLoadFailed: root.featuresOff = []
  }

  // A watch on a file that doesn't exist yet never fires when it appears, and
  // an atomic replace (friday shell style …) can drop it: re-read now and then.
  Timer {
    interval: 2000
    running: true
    repeat: true
    onTriggered: { file.reload(); offFile.reload(); }
  }
}
