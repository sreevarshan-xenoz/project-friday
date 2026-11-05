pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY Shell's look: one palette, one type scale, one motion curve.
//
// The default palette is FRIDAY's own ("Ember on Ink"): warm near-black
// surfaces, an ember accent for FRIDAY and anything it is doing, and a cool
// signal teal for system state. With "palette": "omarchy" in shell.json the
// colours follow the active Omarchy theme (~/.config/omarchy/current/theme/
// colors.toml) and switch live with `omarchy-theme-set`; shapes, type and
// motion stay FRIDAY's.
Singleton {
  id: root

  readonly property var ember: ({
    bg: "#0d0f13",
    surface: "#16191f",
    raised: "#1e222a",
    overlay: "#272c36",
    outline: "#323844",
    text: "#ece8e2",
    subtext: "#a39e96",
    faint: "#6c6862",
    accent: "#ff8f4c",
    onAccent: "#1d0f05",
    signal: "#5fd3bc",
    error: "#ff5d6c",
    warn: "#f5c451",
    ok: "#8bd17c"
  })

  property var omarchy: ({})
  readonly property bool followOmarchy: Config.palette === "omarchy" && omarchy.background !== undefined

  function c(key) {
    if (!root.followOmarchy)
      return root.ember[key];
    const o = root.omarchy;
    const map = {
      bg: o.background,
      surface: o.lighter_background || o.background,
      raised: o.selection || o.lighter_background,
      overlay: o.muted || o.selection,
      outline: o.muted || o.selection,
      text: o.bright_foreground || o.foreground,
      subtext: o.foreground,
      faint: o.dark_foreground || o.foreground,
      accent: o.accent || o.blue,
      onAccent: o.background,
      signal: o.cyan || o.green,
      error: o.red,
      warn: o.yellow,
      ok: o.green
    };
    return map[key] || root.ember[key];
  }

  readonly property color bg: c("bg")
  readonly property color surface: c("surface")
  readonly property color raised: c("raised")
  readonly property color overlay: c("overlay")
  readonly property color outline: c("outline")
  readonly property color text: c("text")
  readonly property color subtext: c("subtext")
  readonly property color faint: c("faint")
  readonly property color accent: c("accent")
  readonly property color onAccent: c("onAccent")
  readonly property color signal: c("signal")
  readonly property color error: c("error")
  readonly property color warn: c("warn")
  readonly property color ok: c("ok")

  function alpha(col, a) { return Qt.rgba(col.r, col.g, col.b, a); }

  // Type: Noto Sans for words, JetBrains Mono for numbers and glyphs.
  readonly property string sans: "Noto Sans"
  readonly property string mono: "JetBrainsMono Nerd Font Propo"
  readonly property int fontS: 11
  readonly property int fontM: 13
  readonly property int fontL: 16
  readonly property int fontXL: 22

  // Geometry. The bar is flush with the top edge and flows into the screen
  // through two concave corners; drawers grow out of it the same way.
  readonly property int barHeight: 36
  readonly property int fillet: 16
  readonly property int radius: 20
  readonly property int radiusS: 10
  readonly property int gap: 8
  readonly property int pad: 16

  // Motion: one decelerating curve, three durations.
  readonly property int fast: 140
  readonly property int medium: 260
  readonly property int slow: 420
  readonly property int easing: Easing.OutCubic

  // ~/.config/omarchy/current/theme/colors.toml: flat `key = "value"` lines.
  FileView {
    path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config")
      + "/omarchy/current/theme/colors.toml"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      const out = {};
      for (const line of text().split("\n")) {
        const m = line.match(/^\s*([A-Za-z_]+)\s*=\s*"(#[0-9A-Fa-f]{6,8})"/);
        if (m)
          out[m[1]] = m[2];
      }
      root.omarchy = out;
    }
    onLoadFailed: root.omarchy = ({})
  }
}
