pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The first backlight in /sys/class/backlight, read from sysfs and set with
// brightnessctl. `ready` is false on machines without one (desktops, VMs).
Singleton {
  id: root

  property string device: ""
  property int max: 0
  property int raw: 0
  // False until the first reading, which is the starting level, not a change
  // (raw can't stand in for this: a screen turned all the way down reads 0).
  property bool known: false
  readonly property bool ready: device !== "" && max > 0
  readonly property real value: ready ? raw / max : 0

  // Fires when the level changes outside the shell (keys), for the OSD.
  signal changedExternally()

  Process {
    running: true
    command: ["sh", "-c", "for d in /sys/class/backlight/*; do [ -r \"$d/max_brightness\" ] && { basename \"$d\"; cat \"$d/max_brightness\"; break; }; done"]
    stdout: StdioCollector {
      onStreamFinished: {
        const lines = text.trim().split("\n");
        if (lines.length === 2 && Number(lines[1]) > 0) {
          root.max = Number(lines[1]);
          root.device = lines[0];
        }
      }
    }
  }

  FileView {
    id: level
    path: root.device ? "/sys/class/backlight/" + root.device + "/brightness" : ""
    printErrors: false
    onLoaded: {
      const t = text().trim();
      const v = Number(t);
      // An empty read is not "0" (Number("") is 0): wait for a real reading.
      if (t === "" || isNaN(v))
        return;
      if (!root.known) {
        root.known = true;
        root.raw = v;
      } else if (v !== root.raw) {
        root.raw = v;
        if (!root.setting)
          root.changedExternally();
      }
    }
  }

  // sysfs attributes don't raise inotify events, so poll while there is one.
  Timer {
    interval: 400
    running: root.device !== ""
    repeat: true
    onTriggered: level.reload()
  }

  property bool setting: false
  Timer { id: settle; interval: 600; onTriggered: root.setting = false }

  function set(v) {
    if (!ready)
      return;
    const pct = Math.round(Math.max(0.01, Math.min(1, v)) * 100);
    root.setting = true;
    settle.restart();
    root.raw = Math.round(pct / 100 * max);
    Quickshell.execDetached(["brightnessctl", "-q", "-d", device, "set", pct + "%"]);
  }
}
