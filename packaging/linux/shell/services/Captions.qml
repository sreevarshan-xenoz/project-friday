pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Live captions (`friday captions`): the last lines of what was heard, drawn
// by CaptionOverlay. `friday-ctl listen captions` sends them:
//   qs ipc call captions put <text>
// They fade a few seconds after the last words; `captions clear` removes them.
Singleton {
  id: root

  property string text: ""
  property bool shown: false

  function put(text: string): void {
    root.text = text;
    root.shown = text !== "";
    fade.restart();
  }

  Timer { id: fade; interval: 8000; onTriggered: root.shown = false }

  IpcHandler {
    target: "captions"
    function put(text: string): void { root.put(text); }
    function clear(): void { root.shown = false; }
  }
}
