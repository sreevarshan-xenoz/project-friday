pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY's standing tasks, from the file the daemon keeps
// (~/.local/state/friday/standing.json). Changes go through `friday tasks`,
// the one place that edits the file, so the daemon and this list agree.
Singleton {
  id: root

  readonly property string file: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
    + "/friday/standing.json"
  // [{ id, goal, when, paused }], in the order they were made.
  property var entries: []

  function pad(n) { return (n < 10 ? "0" : "") + n; }

  // "every day at 08:00", "every 2 h", "battery under 20%".
  function describe(t) {
    if (t.kind === "daily")
      return "every " + (t.weekdays_only ? "weekday" : "day") + " at " + pad(t.hour) + ":" + pad(t.minute);
    if (t.kind === "every")
      return t.minutes % 60 === 0 ? "every " + (t.minutes / 60) + " h" : "every " + t.minutes + " min";
    if (t.kind === "battery_below")
      return "battery under " + t.percent + "%";
    if (t.kind === "once") {
      const d = new Date(t.at * 1000);
      return "once at " + pad(d.getHours()) + ":" + pad(d.getMinutes()) + " on " + d.toLocaleDateString(Qt.locale(), "d MMM");
    }
    return "";
  }

  function parse(text) {
    try {
      const tasks = (JSON.parse(String(text || "{}")).tasks) || [];
      return tasks.filter(t => t && typeof t.id === "string" && typeof t.goal === "string" && t.trigger)
        .map(t => ({ id: t.id, goal: t.goal, when: root.describe(t.trigger), paused: t.paused === true }));
    } catch (e) {
      return [];
    }
  }

  function setPaused(id, paused) { Quickshell.execDetached(["friday", "tasks", paused ? "pause" : "resume", id]); }
  function remove(id) { Quickshell.execDetached(["friday", "tasks", "delete", id]); }

  // For checks and scripts: how many tasks, and the first one's text.
  IpcHandler {
    target: "tasks"
    function count(): int { return root.entries.length; }
    function first(): string {
      const e = root.entries[0];
      return e ? e.when + " | " + e.goal + (e.paused ? " | paused" : "") : "";
    }
  }

  FileView {
    id: view
    path: root.file
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.entries = root.parse(text())
    onLoadFailed: root.entries = []
  }
}
