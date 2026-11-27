import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components
import qs.services

// Small tools in the bar (switch: bar-tools): snip a part of the screen to
// the clipboard, pick a colour from the screen (its hex is copied), and mute
// or unmute the microphone. An idea from end-4/dots-hyprland's bar utility
// buttons (see CREDITS.md).
RowLayout {
  visible: Config.featureOn("bar-tools")
  spacing: 4

  IconButton {
    tooltip: "Snip a part of the screen (copied)"
    icon: Icons.snip
    size: 24
    iconSize: 14
    onClicked: Quickshell.execDetached(["sh", "-c",
      "g=$(slurp) || exit 0; grim -g \"$g\" - | wl-copy --type image/png && notify-send -a FRIDAY -i edit-copy 'Snip copied' 'Paste it anywhere.'"])
  }
  IconButton {
    tooltip: "Pick a colour from the screen (its hex is copied)"
    icon: Icons.colourPick
    size: 24
    iconSize: 14
    onClicked: Quickshell.execDetached(["sh", "-c",
      "command -v hyprpicker >/dev/null || { notify-send -a FRIDAY 'Colour picker' 'It needs hyprpicker (sudo pacman -S hyprpicker).'; exit 0; }; c=$(hyprpicker -a -n) && [ -n \"$c\" ] && notify-send -a FRIDAY 'Colour copied' \"$c\""])
  }
  IconButton {
    visible: Audio.micReady
    tooltip: Audio.micMuted ? "Microphone off: click to turn it on" : "Microphone on: click to mute it"
    icon: Audio.micMuted ? Icons.micOff : Icons.mic
    // Muted: filled red, so it's seen at a glance.
    active: Audio.micMuted
    tint: Theme.error
    size: 24
    iconSize: 14
    onClicked: Audio.toggleMic()
  }
}
