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

  // keys: shown as keycaps. action: a button that runs `command` in a terminal.
  readonly property var steps: [
    { title: "Hello" + (root.name ? ", " + root.name : "") + ". I'm FRIDAY.",
      keys: "Super+Y",
      body: "Press it now and ask me anything. Tap to type; once voice is set up, hold it and speak.",
      waits: true },
    { title: "Ask about what you're reading",
      keys: "Super+Alt+Y",
      body: "Select some text anywhere, then press this and pick Summarize or Explain.",
      waits: true },
    { title: "Ask about your screen",
      keys: "Super+Ctrl+Y",
      body: "Ask about the window in front of you, like \"what does this error mean?\". A screenshot goes to your AI provider only when you press it.",
      waits: true },
    { title: "Let me talk",
      keys: "",
      body: "I can read my answers out loud, in a voice made on this computer (about 100 MB, nothing is sent anywhere). Skip this if you prefer quiet.",
      action: { label: "Give me a voice", command: "friday voice speak setup; friday voice speak on" },
      waits: false },
    { title: "The rest of your desktop",
      keys: "",
      body: "Tap Super for apps · Super+N for controls and notifications · Super+L to lock · Super+K for every key. Take this tour again any time with: friday tour",
      waits: false }
  ].concat(root.phone ? [
    { title: "Your phone",
      keys: "",
      body: "Pocketdesk shows this computer on your phone and lets it answer FRIDAY's questions. Pairing shows a PIN to type on the phone.",
      action: { label: "Pair my phone", command: "friday phone on" },
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
  function back() { if (root.step > 0) root.step--; }
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
    function back(): void { root.back(); }
    function finish(): void { root.finish(); }
  }
}
