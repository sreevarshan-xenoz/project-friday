pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Finds the image for a desktop entry's Icon= value. Qt's themed lookup only
// sees the active icon theme, which on a bare Hyprland session is just
// hicolor, so web apps and most installed apps came up blank. This keeps an
// index of every app and device icon in the XDG icon dirs and pixmaps
// (scalable first), and resolves in order: absolute path or URL, the index,
// Qt's themed lookup. "" means no icon; callers draw their own fallback.
Singleton {
  id: root

  property var index: ({})
  property var pending: ({})

  function source(icon) {
    const value = String(icon || "");
    if (value === "")
      return "";
    if (value.startsWith("file://") || value.startsWith("image://"))
      return value;
    if (value.startsWith("/"))
      return "file://" + value;
    const hit = root.index[value];
    if (hit)
      return "file://" + hit;
    return Quickshell.iconPath(value, true);
  }

  // Rebuilt when the shell starts and whenever the set of apps changes (an
  // install drops in new icons); bursts collapse into one scan.
  function rescan() { debounce.restart(); }

  Timer {
    id: debounce
    interval: 1500
    onTriggered: if (!scan.running) scan.running = true
  }

  Connections {
    target: DesktopEntries.applications
    function onValuesChanged() { root.rescan(); }
  }

  Process {
    id: scan
    running: true
    // SVGs are listed before PNGs, and larger PNG sizes before smaller ones;
    // the first hit per name wins.
    command: ["sh", "-c", "dirs=\"$HOME/.icons $HOME/.local/share/icons\"; "
      + "for d in $(printf %s \"${XDG_DATA_DIRS:-/usr/local/share:/usr/share}\" | tr : ' '); do dirs=\"$dirs $d/icons\"; done; "
      + "for b in $dirs; do [ -d \"$b\" ] && find \"$b\" \\( -path '*/apps/*' -o -path '*/devices/*' \\) -name '*.svg' 2>/dev/null; done; "
      + "for b in $dirs; do [ -d \"$b\" ] && find \"$b\" \\( -path '*/apps/*' -o -path '*/devices/*' \\) -name '*.png' 2>/dev/null; done | sort -rV; "
      + "find /usr/share/pixmaps -maxdepth 1 \\( -name '*.svg' -o -name '*.png' \\) 2>/dev/null"]
    stdout: SplitParser {
      onRead: line => {
        const path = line.trim();
        const name = path.slice(path.lastIndexOf("/") + 1).replace(/\.(svg|png)$/, "");
        if (name !== "" && root.pending[name] === undefined)
          root.pending[name] = path;
      }
    }
    onStarted: root.pending = {}
    onExited: root.index = root.pending
  }
}
