import QtQuick
import Quickshell.Io

// Runs `friday <args>` and keeps what it printed (errors too).
Item {
  id: root

  property var args: []
  property string output: ""
  readonly property bool running: proc.running
  signal finished(string output)

  function run(extra) {
    if (proc.running) return;
    root.output = "";
    proc.command = ["bash", "-c", "friday \"$@\" 2>&1", "friday"].concat(root.args, extra || []);
    proc.running = true;
  }

  Process {
    id: proc
    stdout: StdioCollector {
      onStreamFinished: {
        root.output = text.trim();
        root.finished(root.output);
      }
    }
  }
}
