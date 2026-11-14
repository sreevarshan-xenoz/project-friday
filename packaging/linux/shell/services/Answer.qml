pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// What the answer pill shows. It wakes up on its own while FRIDAY works on a
// goal (state.json, via Friday), streaming the answer as it arrives; `friday
// ask` then hands it the finished answer over IPC, with the same wording the
// notification would have had:
//   qs ipc call answer present <ok> <goal> <answer> <meta> <mode>
// (not `show`: the qs CLI reads that as its own `ipc show`)
// `friday ask` falls back to a notification when nothing answers that call.
Singleton {
  id: root

  // idle | listening | working | answer
  property string state: "idle"
  property bool ok: true
  property string goal: ""
  property string body: ""
  property string meta: ""
  // How it was asked (`--selection`, `--screen`, `--region`, or ""), so Ask
  // again asks the same way.
  property string mode: ""
  // Bumped on every new answer, so the pill restarts its timers.
  property int serial: 0

  readonly property bool visible: state !== "idle"

  // A goal FRIDAY starts from anywhere (Super+Y, the drawer, the CLI) opens the
  // pill; the streamed text follows state.json's partial answer.
  Connections {
    target: Friday
    function onWorkingChanged() {
      if (Friday.working) {
        root.state = "working";
        root.goal = Friday.goal;
        root.body = Friday.partial;
        root.meta = "";
        root.serial++;
      } else if (root.state === "working") {
        settle.restart();
      }
    }
    function onGoalChanged() { if (root.state === "working" && Friday.goal) root.goal = Friday.goal; }
    function onPartialChanged() { if (root.state === "working" && Friday.partial) root.body = Friday.partial; }
  }

  // `friday ask` hands over the answer right after the goal ends; a goal
  // started elsewhere (the FRIDAY drawer) doesn't, so show how it ended.
  Timer {
    id: settle
    interval: 2500
    onTriggered: {
      if (root.state !== "working" || Friday.working)
        return;
      root.show(!Friday.lastFailed, Friday.lastGoal || root.goal, Friday.lastDetail || root.body, "");
    }
  }

  function show(ok, goal, body, meta, mode) {
    settle.stop();
    root.mode = mode || "";
    root.ok = ok;
    root.goal = goal;
    root.body = body;
    root.meta = meta;
    root.state = "answer";
    root.serial++;
  }

  function dismiss() { root.state = "idle"; }

  // Super+Y held with voice on: shown until the words are asked.
  function listening() {
    settle.stop();
    root.ok = true;
    root.goal = "";
    root.body = "";
    root.meta = "";
    root.state = "listening";
    root.serial++;
  }

  function copy() { Quickshell.execDetached(["wl-copy", "--", root.body]); }
  // Types the answer into the window that had focus, replacing a selection
  // there; the short wait lets focus go back to it first. Line breaks become
  // spaces: typed, each one is an Enter (a terminal would run every line).
  function typeIt() {
    const text = root.body.replace(/\s*\n\s*/g, " ").trim();
    root.dismiss();
    Quickshell.execDetached(["sh", "-c", "sleep 0.35; exec wtype -- \"$1\"", "sh", text]);
  }
  function followUp(question) {
    const q = String(question || "").trim();
    if (q) Quickshell.execDetached(["friday", "ask", "--follow-up", q]);
  }
  function askAgain() {
    const args = ["friday", "ask"];
    if (["--selection", "--screen", "--region"].indexOf(root.mode) !== -1)
      args.push(root.mode);
    args.push(root.goal);
    Quickshell.execDetached(args);
  }
  function openFull() { Quickshell.execDetached(["foot", "-H", "-T", "FRIDAY · " + root.goal, "friday", "answers", "--last"]); }

  IpcHandler {
    target: "answer"
    function present(ok: bool, goal: string, body: string, meta: string, mode: string): void { root.show(ok, goal, body, meta, mode); }
    function dismiss(): void { root.dismiss(); }
    function listening(): void { root.listening(); }
    function state(): string { return root.state; }
  }
}
