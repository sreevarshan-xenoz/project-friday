pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.common

// How busy the computer is: CPU and memory in use, 0 to 1, sampled every
// 3 s from /proc while the bar shows them (switch: bar-resources), and not
// at all otherwise. `hot` means over end-4's warning lines (CPU 90%,
// memory 95%).
Singleton {
  id: root

  readonly property bool on: Config.featureOn("bar-resources")
  property real cpu: 0
  property real mem: 0
  readonly property bool cpuHot: cpu >= 0.9
  readonly property bool memHot: mem >= 0.95
  property var last: null

  FileView { id: stat; path: "/proc/stat" }
  FileView { id: meminfo; path: "/proc/meminfo" }
  Timer {
    interval: 3000
    running: root.on
    repeat: true
    triggeredOnStart: true
    onTriggered: {
      stat.reload();
      meminfo.reload();
      const f = (stat.text().split("\n")[0] || "").trim().split(/\s+/).slice(1).map(Number);
      const idle = (f[3] || 0) + (f[4] || 0), total = f.reduce((a, b) => a + b, 0);
      if (root.last && total > root.last.total)
        root.cpu = Math.max(0, Math.min(1, 1 - (idle - root.last.idle) / (total - root.last.total)));
      root.last = { idle: idle, total: total };
      const m = meminfo.text();
      const kb = k => Number((m.match(new RegExp("^" + k + ":\\s+(\\d+)", "m")) || [0, 0])[1]);
      if (kb("MemTotal") > 0)
        root.mem = 1 - kb("MemAvailable") / kb("MemTotal");
    }
  }
}
