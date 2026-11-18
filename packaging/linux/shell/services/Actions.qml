pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// What FRIDAY did on the desktop, from the log the daemon keeps
// (~/.local/state/friday/actions.jsonl; `friday actions` prints the same).
// Which action, with what, whether it worked; never what was on the screen.
Singleton {
  id: root

  readonly property string file: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
    + "/friday/actions.jsonl"
  // [{ text, ok, when }], newest first, at most `keep`.
  property var entries: []
  readonly property int keep: 5

  // The first plain word the action was given ("firefox", "workspace 2").
  function subject(params) {
    if (!params || typeof params !== "object") return "";
    for (const k of ["target", "app", "query", "command", "workspace", "name", "label"]) {
      if (params[k] !== undefined && params[k] !== null && String(params[k]) !== "")
        return String(params[k]);
    }
    return "";
  }

  function parse(text) {
    const out = [];
    for (const line of String(text || "").split("\n")) {
      if (line.trim() === "") continue;
      try {
        const a = JSON.parse(line);
        if (!a || typeof a.verb !== "string") continue;
        const s = root.subject(a.params);
        const d = new Date(a.ts);
        out.push({
          text: a.verb.replace(/_/g, " ") + (s !== "" ? " · " + s : ""),
          ok: a.ok === true,
          when: isNaN(d.getTime()) ? "" : Qt.formatTime(d, "HH:mm")
        });
      } catch (e) { /* a torn last line */ }
    }
    return out.slice(-root.keep).reverse();
  }

  // For checks and scripts: how many entries, and the newest one's text.
  IpcHandler {
    target: "actions"
    function count(): int { return root.entries.length; }
    function first(): string {
      const e = root.entries[0];
      return e ? e.text + (e.ok ? "" : " | failed") : "";
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
