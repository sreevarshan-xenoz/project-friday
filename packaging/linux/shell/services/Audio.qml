pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Pipewire
import qs.common

// Default output and input through PipeWire. `ready` is false with no
// PipeWire (or no default device), and the UI hides what it can't control.
Singleton {
  id: root

  readonly property PwNode sink: Pipewire.defaultAudioSink
  readonly property PwNode source: Pipewire.defaultAudioSource

  PwObjectTracker {
    objects: [root.sink, root.source].filter(n => n !== null)
  }

  readonly property bool ready: sink !== null && sink.audio !== null
  readonly property real volume: ready ? sink.audio.volume : 0
  readonly property bool muted: ready ? sink.audio.muted : false
  readonly property bool micReady: source !== null && source.audio !== null
  readonly property bool micMuted: micReady ? source.audio.muted : false

  function setVolume(v) {
    if (ready)
      sink.audio.volume = Math.max(0, Math.min(1, v));
  }
  function toggleMute() {
    if (ready)
      sink.audio.muted = !sink.audio.muted;
  }
  function toggleMic() {
    if (micReady)
      source.audio.muted = !source.audio.muted;
  }

  function icon() {
    if (!ready || muted || volume <= 0.001)
      return Icons.volumeOff;
    return volume < 0.34 ? Icons.volumeLow : volume < 0.67 ? Icons.volumeMid : Icons.volumeHigh;
  }
}
