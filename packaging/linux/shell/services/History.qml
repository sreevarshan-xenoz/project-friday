pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY's past answers, newest first, from the file `friday ask` keeps
// (~/.local/state/friday/answers.md: "# question", "_date_", an optional
// "> context" line, the answer, then "---"). Read-only; private to the user.
Singleton {
  id: root

  readonly property string file: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
    + "/friday/answers.md"
  // [{ goal, when, body }], newest first, the last 300.
  property var entries: []

  function parse(text) {
    const out = [];
    const blocks = String(text || "").split(/\n---\n/);
    for (let i = blocks.length - 1; i >= 0 && out.length < 300; i--) {
      const lines = blocks[i].trim().split("\n");
      if (lines.length < 1 || !lines[0].startsWith("# "))
        continue;
      let k = 1;
      while (k < lines.length && lines[k].trim() === "") k++;
      let when = "";
      const m = k < lines.length ? lines[k].match(/^_(.+)_$/) : null;
      if (m) { when = m[1]; k++; }
      while (k < lines.length && (lines[k].trim() === "" || lines[k].startsWith("> "))) k++;
      let body = lines.slice(k).join("\n");
      const details = body.indexOf("\n<details>");
      if (details !== -1) body = body.slice(0, details);
      out.push({ goal: lines[0].slice(2), when: when, body: body.trim() });
    }
    return out;
  }

  // Up to `limit` answers whose question or answer has every word of `query`.
  function search(query, limit) {
    const words = String(query || "").toLowerCase().split(/\s+/).filter(w => w.length > 1);
    if (words.length === 0)
      return [];
    const hits = [];
    for (const e of root.entries) {
      const hay = (e.goal + "\n" + e.body).toLowerCase();
      if (words.every(w => hay.indexOf(w) !== -1)) {
        hits.push(e);
        if (hits.length >= (limit || 5)) break;
      }
    }
    return hits;
  }

  // For checks and scripts: how many answers, and the newest match's question.
  IpcHandler {
    target: "history"
    function count(): int { return root.entries.length; }
    function find(query: string): string { const h = root.search(query, 1); return h.length ? h[0].goal : ""; }
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
