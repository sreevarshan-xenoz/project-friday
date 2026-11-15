pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The first-login tour: a few steps that ask you to press FRIDAY's keys
// rather than read about them. `friday welcome` starts it once:
//   qs ipc call tour begin <name> <onFreeModels> <pocketdeskInstalled>
// The first steps move on by themselves when an answer arrives.
Singleton {
  id: root

  property bool active: false
  property int step: 0
  property string name: ""
  property bool freeModels: false
  property bool phone: false

  readonly property var steps: [
    { title: "Hello" + (root.name ? ", " + root.name : "") + ". I'm FRIDAY.",
      body: "Press Super+Y now and ask me anything. Tap it to type; once voice is set up, hold it and speak.",
      waits: true },
    { title: "Ask about what you're reading",
      body: "Select some text anywhere, then press Super+Alt+Y and pick Summarize or Explain.",
      waits: true },
    { title: "Ask about your screen",
      body: "Super+Ctrl+Y asks about the window in front of you. A screenshot goes to your AI provider only when you press it.",
      waits: false },
    { title: "The rest of your desktop",
      body: "Tap Super for apps · Super+N for controls and notifications · Super+L to lock · Super+K for every key.",
      waits: false }
  ].concat(root.phone ? [
    { title: "Your phone",
      body: "Pocketdesk shows this computer on your phone and lets it answer FRIDAY's questions. To pair one, run: friday phone on",
      waits: false }
  ] : [])
  readonly property var current: steps[Math.min(step, steps.length - 1)]
  readonly property bool last: step >= steps.length - 1

  function begin(name, freeModels, phone) {
    root.name = name;
    root.freeModels = freeModels;
    root.phone = phone;
    root.step = 0;
    root.active = true;
  }
  function next() { if (root.last) root.finish(); else root.step++; }
  function finish() { root.active = false; }

  // A step that waits moves on once FRIDAY has answered.
  Connections {
    target: Answer
    function onStateChanged() {
      if (root.active && root.current.waits && Answer.state === "answer")
        root.next();
    }
  }

  IpcHandler {
    target: "tour"
    function begin(name: string, freeModels: bool, phone: bool): void { root.begin(name, freeModels, phone); }
    function step(): int { return root.active ? root.step : -1; }
    function next(): void { root.next(); }
    function finish(): void { root.finish(); }
  }
}
