pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.common

// Connection summary from NetworkManager's nmcli, polled. Without nmcli (or
// NetworkManager) `ready` stays false and the bar shows nothing for it.
Singleton {
  id: root

  property bool ready: false
  property string kind: ""        // "wifi" | "ethernet" | ""
  property string name: ""        // SSID or connection name
  property int strength: 0        // 0-100 for Wi-Fi
  property bool wifiEnabled: false

  readonly property bool connected: kind !== ""

  function icon() {
    if (kind === "ethernet")
      return Icons.ethernet;
    if (kind === "wifi")
      return strength >= 55 ? Icons.wifi : Icons.wifiWeak;
    return Icons.wifiOff;
  }

  function refresh() { if (!poll.running) poll.running = true; }

  function setWifi(on) {
    Quickshell.execDetached(["nmcli", "radio", "wifi", on ? "on" : "off"]);
    root.wifiEnabled = on;
    delayed.restart();
  }
  function settings() { Quickshell.execDetached(["foot", "-T", "Network", "nmtui"]); }

  Timer { id: delayed; interval: 1500; onTriggered: root.refresh() }
  Timer { interval: 8000; running: true; repeat: true; triggeredOnStart: true; onTriggered: root.refresh() }

  // One call: the radio state, then each active device, then Wi-Fi signal.
  Process {
    id: poll
    command: ["sh", "-c", "command -v nmcli >/dev/null || exit 3; nmcli -t radio wifi; echo --; "
      + "nmcli -t -f TYPE,STATE,CONNECTION device; echo --; nmcli -t -f ACTIVE,SIGNAL dev wifi list --rescan no 2>/dev/null"]
    stdout: StdioCollector {
      onStreamFinished: {
        const parts = text.split("--\n");
        if (parts.length < 2) {
          root.ready = false;
          return;
        }
        root.ready = true;
        root.wifiEnabled = parts[0].trim() === "enabled";
        let kind = "", name = "";
        for (const line of parts[1].split("\n")) {
          const f = line.split(":");
          if (f.length < 3 || f[1] !== "connected")
            continue;
          if (f[0] === "ethernet" && kind !== "ethernet") { kind = "ethernet"; name = f.slice(2).join(":"); }
          else if (f[0] === "wifi" && kind === "") { kind = "wifi"; name = f.slice(2).join(":"); }
        }
        let strength = 0;
        for (const line of (parts[2] || "").split("\n")) {
          const f = line.split(":");
          if (f[0] === "yes") strength = Number(f[1]) || 0;
        }
        root.kind = kind;
        root.name = name;
        root.strength = strength;
      }
    }
  }
}
