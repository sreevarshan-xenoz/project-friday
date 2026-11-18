pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris

// The player the bar's media chip shows: what is playing, else a player
// paused on a track, so it can be resumed from the bar.
Singleton {
  id: root

  readonly property var player: {
    const all = Mpris.players.values;
    return all.find(p => p.isPlaying)
      || all.find(p => p.playbackState === MprisPlaybackState.Paused && p.trackTitle)
      || null;
  }
  readonly property bool playing: player !== null && player.isPlaying

  // For checks and scripts: playing | paused | none.
  IpcHandler {
    target: "media"
    function state(): string { return root.player ? (root.playing ? "playing" : "paused") : "none"; }
  }
}
