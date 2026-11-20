pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The launcher's quick commands: things this computer does by itself, with
// nothing sent anywhere (`friday ask` answers them locally; see
// friday_local_command in packaging/linux/friday). Picking one runs it and the
// answer pill says what happened.
Singleton {
  id: root

  // [{ name, hint, words, run }]: `words` are extra things a person might type.
  readonly property var all: [
    { name: "Mute", hint: "Silence the sound", words: "sound audio volume off", run: ["friday", "ask", "mute"] },
    { name: "Unmute", hint: "Bring the sound back", words: "sound audio volume on", run: ["friday", "ask", "unmute"] },
    { name: "Do not disturb: on", hint: "Silence notifications", words: "dnd notifications quiet", run: ["friday", "ask", "dnd on"] },
    { name: "Do not disturb: off", hint: "Show notifications again", words: "dnd notifications", run: ["friday", "ask", "dnd off"] },
    { name: "Battery", hint: "How much is left", words: "power charge", run: ["friday", "ask", "battery"] },
    { name: "Timer: 5 minutes", hint: "A notification when it's up", words: "alarm countdown", run: ["friday", "ask", "timer 5 min"] },
    { name: "Watch an area", hint: "Told when it changes, or stops changing", words: "watch screen progress download notify", run: ["friday", "watch", "region"] },
    { name: "Wi-Fi and Bluetooth", hint: "Open the controls", words: "wifi network internet bluetooth sound", run: ["friday", "ask", "wifi settings"] },
    { name: "FRIDAY Settings", hint: "AI, voice, permissions, privacy", words: "preferences options configure", run: ["friday", "app"] },
    { name: "How is my computer doing?", hint: "A read-only health look", words: "health slow disk memory", run: ["friday", "ask", "How is my computer doing?"] },
    { name: "Take the tour", hint: "The keys, one at a time", words: "help tutorial welcome", run: ["friday", "tour"] }
  ]

  // The commands a query asks for, best first. A query must be at least two
  // letters, so a stray "a" doesn't list everything.
  function match(query) {
    const q = String(query || "").trim().toLowerCase();
    if (q.length < 2) return [];
    const scored = [];
    for (const c of root.all) {
      const name = c.name.toLowerCase();
      const extra = (c.hint + " " + c.words).toLowerCase();
      let s = 0;
      if (name.startsWith(q)) s = 4;
      else if (name.split(/[\s:\-_.?]+/).some(w => w.startsWith(q))) s = 3;
      else if (name.includes(q)) s = 2;
      else if (extra.includes(q)) s = 1;
      if (s > 0) scored.push({ c, s });
    }
    scored.sort((a, b) => b.s - a.s);
    return scored.map(x => x.c);
  }

  // For checks and scripts: the names a query finds, "|"-separated.
  IpcHandler {
    target: "commands"
    function match(query: string): string { return root.match(query).map(c => c.name).join("|"); }
  }
}
