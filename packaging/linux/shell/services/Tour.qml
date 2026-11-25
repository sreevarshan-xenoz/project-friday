pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The first-login tour: a few steps that ask you to press FRIDAY's keys
// rather than read about them. `friday welcome` starts it once:
//   qs ipc call tour begin <name> <onFreeModels> <phoneAppInstalled>
// The first steps move on by themselves when an answer arrives.
Singleton {
  id: root

  property bool active: false
  property int step: 0
  // The step whose ask failed (its card says where to look), or -1.
  property int failedStep: -1
  property string name: ""
  property bool freeModels: false
  property bool phone: false
  // A camera, and face unlock not set up yet: the tour offers it.
  property bool camera: false

  // keys: shown as keycaps. action: a button that runs `command` in a terminal.
  // Offered unless switched off (friday feature app-bundles off).
  property bool bundles: true
  readonly property var steps: ([
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
      body: "Ask about the window in front of you, like \"what does this error mean?\". Its text (or, when the app offers too little, a screenshot) goes to your AI provider only when you press it.",
      waits: true },
    { title: "Let me talk",
      keys: "",
      body: "I can read my answers out loud, in a voice made on this computer (about 50 MB, nothing is sent anywhere). Skip this if you prefer quiet.",
      action: { label: "Give me a voice", command: "friday voice speak setup; friday voice speak on" },
      waits: false }
  ].concat(root.camera ? [
    { title: "Unlock with your face",
      keys: "Super+L",
      body: "The lock screen can know your face (Gaze, checked on this computer; no picture is sent anywhere). Setting it up asks for your password, then has you look at the camera. Typing your password always works too. After that, press Super+L and look at the camera to try it; turn it off with: friday face remove",
      action: { label: "Set up face unlock", command: "friday face setup" },
      waits: false }
  ] : []).concat([
    { title: "Install the apps you use",
      keys: "",
      body: "Pick kinds of apps (developer, gaming, productivity, creative) and FRIDAY installs them in one go, from the Arch repos where it can. One that fails never stops the rest; you get a notification when it's done. See what's in each: friday bundles list",
      action: { label: "Pick apps", command: "friday bundles" },
      waits: false,
      bundles: true },
    { title: "The rest of your desktop",
      keys: "",
      body: "Tap Super for apps · Super+N for controls and notifications · Super+L to lock · Super+K for every key. Take this tour again any time with: friday tour",
      waits: false }
  ]).concat(root.phone ? [
    { title: "Your phone",
      keys: "",
      body: "FRIDAY Mobile shows this computer on your phone and lets it answer FRIDAY's questions. Pairing shows a PIN to type on the phone.",
      action: { label: "Pair my phone", command: "friday phone on" },
      waits: false }
  ] : [])).filter(s => !s.bundles || root.bundles)
  readonly property var current: steps[Math.min(step, steps.length - 1)]
  readonly property bool last: step >= steps.length - 1

  function begin(name, freeModels, phone) {
    root.name = name;
    root.freeModels = freeModels;
    root.phone = phone;
    root.step = 0;
    root.failedStep = -1;
    // The steps are settled before the tour shows, so none appears or
    // shifts while you're on it.
    root.active = false;
    cameraCheck.running = true;
  }

  Process {
    id: bundlesCheck
    command: ["sh", "-c", "friday feature app-bundles status 2>/dev/null | grep -q ': on'"]
    onExited: code => {
      root.bundles = code === 0;
      root.active = true;
    }
  }

  // Only a camera FRIDAY could use, and only while face unlock is off. In
  // tests FRIDAY_VIDEO_DEVS stands in for /dev/video* and FRIDAY_FACE_PAM for
  // the face unlock PAM file.
  Process {
    id: cameraCheck
    command: ["sh", "-c", "[ -e \"${FRIDAY_FACE_PAM:-/etc/pam.d/friday-lock-face}\" ] && exit 1; for d in ${FRIDAY_VIDEO_DEVS:-/dev/video*}; do [ -e \"$d\" ] && exit 0; done; exit 1"]
    onExited: code => {
      root.camera = code === 0;
      bundlesCheck.running = true;
    }
  }
  function next() { if (root.last) root.finish(); else root.step++; }
  function back() { if (root.step > 0) root.step--; }
  function finish() { root.active = false; }

  // A step that waits moves on once FRIDAY has answered; a failed answer stays,
  // so the first question that went wrong isn't skipped past.
  Connections {
    target: Answer
    function onStateChanged() {
      if (!root.active || !root.current.waits || Answer.state !== "answer")
        return;
      if (Answer.ok)
        root.next();
      else
        root.failedStep = root.step;
    }
  }

  IpcHandler {
    target: "tour"
    function begin(name: string, freeModels: bool, phone: bool): void { root.begin(name, freeModels, phone); }
    function count(): int { return root.steps.length; }
    function step(): int { return root.active ? root.step : -1; }
    function next(): void { root.next(); }
    function back(): void { root.back(); }
    function finish(): void { root.finish(); }
  }
}
