pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// What the answer pill shows. It wakes up on its own while FRIDAY works on a
// goal (state.json, via Friday), streaming the answer as it arrives; `friday
// ask` then hands it the finished answer over IPC, with the same wording the
// notification would have had:
//   qs ipc call answer present <ok> <goal> <answer> <meta> <mode> <served>
//   (`served` is the provider/model that answered; "" when unknown)
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
  // The provider/model that gave this answer, so Try another model skips it.
  property string served: ""
  // Bumped on every new answer, so the pill restarts its timers.
  property int serial: 0
  // What went to the model with the question, shown while it works
  // ("firefox · the window's text, 2.1k chars"); "" when nothing did.
  property string receipt: ""

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
        root.receipt = "";
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
      const ok = !Friday.lastFailed;
      root.show(ok, Friday.lastGoal || root.goal, Friday.lastDetail || root.body || (ok ? "Done." : "That didn't finish."), "");
    }
  }

  function show(ok, goal, body, meta, mode, served) {
    settle.stop();
    root.mode = mode || "";
    root.served = served || "";
    root.ok = ok;
    root.goal = goal;
    root.body = body;
    root.meta = meta;
    root.state = "answer";
    root.serial++;
  }

  // `friday ask` says what it sent, before the first word: the pill shows it
  // working on `goal` (a screen ask has no goal in state.json to wake it).
  // A screen ask has no goal in state.json: if no answer comes after its
  // receipt (the ask timed out, FRIDAY stopped), the pill doesn't stay.
  Timer {
    id: sentGiveUp
    interval: 250000
    onTriggered: if (root.state === "working" && !Friday.working) root.dismiss();
  }

  function sent(goal, text) {
    sentGiveUp.restart();
    if (root.state !== "working") {
      settle.stop();
      root.ok = true;
      root.body = "";
      root.meta = "";
      root.state = "working";
      root.serial++;
    }
    if (goal)
      root.goal = goal;
    root.receipt = text;
  }

  function dismiss() { root.state = "idle"; }
  // Put away by the person (Esc, the close button): the reading stops too.
  function close() {
    root.dismiss();
    Quickshell.execDetached(["friday", "say", "--stop"]);
  }

  // What the pill says to do when the key is let go.
  property string listenHint: "let go of Super+Y to ask"

  // Super+Y held with voice on: shown until the words are asked. Super+Shift+Y
  // (voice typing): shown until the words are typed.
  function listening(typing) {
    root.listenHint = typing === true ? "let go of Super+Shift+Y to type" : "let go of Super+Y to ask";
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
  // Whether another model can be asked: a question about text, not a
  // screenshot (those go through the screen-reading path).
  readonly property bool canTryAnother: root.goal !== "" && root.mode !== "--screen" && root.mode !== "--region"
  // The same question again, answered with more care by a different model.
  function tryAnother() {
    const args = ["friday", "ask"];
    if (root.mode === "--selection") args.push("--selection");
    args.push("--another", root.served || "none/none", root.goal);
    Quickshell.execDetached(args);
  }
  // A repro of this answer, no question or answer text (friday wrong).
  function markWrong() { Quickshell.execDetached(["friday", "wrong"]); }
  function openFull() { Quickshell.execDetached(["foot", "-H", "-T", "FRIDAY · " + root.goal, "friday", "answers", "--last"]); }

  IpcHandler {
    target: "answer"
    function present(ok: bool, goal: string, body: string, meta: string, mode: string, served: string): void { root.show(ok, goal, body, meta, mode, served); }
    function dismiss(): void { root.dismiss(); }
    function receipt(goal: string, text: string): void { root.sent(goal, text); }
    function listening(): void { root.listening(false); }
    function dictating(): void { root.listening(true); }
    function dictatingLatched(): void { root.listening(true); root.listenHint = "press Super+Shift+Y again to type it"; }
    function state(): string { return root.state; }
  }
}
