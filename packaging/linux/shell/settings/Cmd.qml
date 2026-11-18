import QtQuick
import Quickshell.Io

// Runs `friday <args>` and keeps what it printed (errors too).
Item {
  id: root

  // Not drawn, and not a layout item: a page's spacing would otherwise add a gap for each command.
  visible: false

  property var args: []
  property string output: ""
  readonly property bool running: proc.running
  signal finished(string output)

  // `input` is written to the command's stdin (a pasted key never goes on the command line).
  function run(extra, input) {
    if (proc.running) return;
    root.output = "";
    root.pending = input === undefined ? "" : input + "\n";
    proc.command = ["bash", "-c", "friday \"$@\" 2>&1", "friday"].concat(root.args, extra || []);
    proc.running = true;
  }

  property string pending: ""

  Process {
    id: proc
    stdinEnabled: true
    onStarted: {
      if (root.pending !== "") { proc.write(root.pending); root.pending = ""; }
    }
    stdout: StdioCollector {
      onStreamFinished: {
        root.output = text.trim();
        root.finished(root.output);
      }
    }
  }
}
