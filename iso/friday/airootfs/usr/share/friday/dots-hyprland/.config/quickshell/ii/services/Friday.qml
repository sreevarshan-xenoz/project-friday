pragma Singleton

import Quickshell
import Quickshell.Io
import QtQuick

/**
 * Read-only view of the FRIDAY daemon's state.json for the ii shell.
 *
 * The daemon writes the file atomically (tmp + rename); FileView picks up each
 * write. Every accessor is guarded so a missing file, malformed JSON or a
 * vanished field degrades to "offline"/empty instead of a QML error. This
 * mirrors friday-os/plugins/friday.shell/FridayState.qml (the Omarchy bar
 * widget), which reads the same contract.
 */
Singleton {
    id: root

    readonly property string stateFile: {
        const base = Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state");
        return base + "/friday/state.json";
    }

    property var parsed: ({})

    function str(v) { return (v === undefined || v === null) ? "" : String(v); }
    function num(v) { const n = Number(v); return isNaN(n) ? -1 : n; }

    function load() {
        const text = fileView.text();
        if (!text) { root.parsed = {}; return; }
        try {
            const obj = JSON.parse(text);
            root.parsed = (obj && typeof obj === "object") ? obj : {};
        } catch (e) {
            root.parsed = {};
        }
    }

    FileView {
        id: fileView
        path: root.stateFile
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: root.load()
        onLoadFailed: root.parsed = ({})
    }

    // Fallback: an atomic rename can drop the file watch, and the file may not
    // exist yet when the shell starts before the daemon. Re-read cheaply.
    Timer {
        interval: 3000
        running: true
        repeat: true
        onTriggered: fileView.reload()
    }

    readonly property string status: str(parsed.status)
    readonly property bool present: status !== ""
    readonly property bool running: status === "running"

    readonly property var current: (parsed.current && typeof parsed.current === "object") ? parsed.current : null
    readonly property bool working: running && current !== null && current.active === true
    readonly property string goal: current ? str(current.goal) : ""
    readonly property string phase: current ? str(current.phase) : ""

    readonly property var lastGoal: (parsed.last_goal && typeof parsed.last_goal === "object") ? parsed.last_goal : null
    readonly property string lastOutcome: lastGoal ? str(lastGoal.outcome) : ""
    readonly property string lastText: lastGoal ? str(lastGoal.goal) : ""
    readonly property string lastDetail: lastGoal ? str(lastGoal.result || lastGoal.error || lastGoal.failed_reason) : ""
    readonly property bool lastFailed: /fail|error|abort/i.test(lastOutcome)

    readonly property string voice: str(parsed.voice)
    readonly property bool voiceOn: voice !== "" && voice !== "off"

    /** Coarse mode the bar animates between. */
    readonly property string mode: !present ? "offline"
        : !running ? "degraded"
        : working ? "working"
        : "idle"

    /** Open the `friday ask` prompt (same as SUPER+Y). */
    function ask() {
        Quickshell.execDetached(["friday", "ask"]);
    }

    /** Cancel the goal in flight. */
    function cancel() {
        Quickshell.execDetached(["friday", "cancel"]);
    }

    /** (Re)start the user daemon; state.json updates once it is up. */
    function startDaemon() {
        Quickshell.execDetached(["systemctl", "--user", "restart", "friday-linux-daemon"]);
    }

    /** Health check in a terminal that stays open (`foot -H`). */
    function doctor() {
        Quickshell.execDetached(["foot", "-H", "-T", "FRIDAY doctor", "friday", "doctor"]);
    }
}
