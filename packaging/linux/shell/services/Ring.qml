pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// "Show me": a ring around a spot on the screen, with a short label, drawn
// by RingOverlay on the screen that holds it. `friday show` sends it:
//   qs ipc call ring point <x> <y> <width> <height> <label>   (global pixels)
// It fades after a few seconds; `ring hide` clears it sooner. (Not "show":
// quickshell reads that as its own `ipc show`.)
Singleton {
  id: root

  property int x: 0
  property int y: 0
  property int width: 0
  property int height: 0
  property string label: ""
  property bool shown: false

  function show(x: int, y: int, width: int, height: int, label: string): void {
    root.x = x;
    root.y = y;
    root.width = Math.max(width, 8);
    root.height = Math.max(height, 8);
    root.label = label;
    root.shown = true;
    fade.restart();
  }

  Timer { id: fade; interval: 6000; onTriggered: root.shown = false }

  IpcHandler {
    target: "ring"
    function point(x: int, y: int, width: int, height: int, label: string): void { root.show(x, y, width, height, label); }
    function hide(): void { root.shown = false; }
    function visible(): bool { return root.shown; }
  }
}
