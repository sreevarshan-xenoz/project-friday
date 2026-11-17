pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY Shell's look: a palette, a style, one type scale, one motion curve.
//
// Palettes (shell.json "palette"): FRIDAY's own, each with an accent for
// FRIDAY and anything it is doing and a signal colour for system state:
//   ember  warm near-black, ember accent, teal signal (the default)
//   frost  cool slate, ice-blue accent, lilac signal
//   moss   dark olive, lichen-green accent, amber signal
//   dawn   light: warm paper, rust accent, deep-teal signal
// or "classic": follow the active desktop theme ($XDG_STATE_HOME/omarchy/
// current/theme/colors.toml, where omarchy-theme-set links it), live.
//
// Styles (shell.json "style") change where the bar sits and its shape:
//   ember   flush with the top edge, flowing into the screen through
//           concave corners; drawers grow out of it
//   island  three floating pills along the top; drawers are floating cards
//   dock    one floating bar along the bottom; drawers rise from it
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

  readonly property var frost: ({
    bg: "#0e141b",
    surface: "#151d27",
    raised: "#1c2633",
    overlay: "#243142",
    outline: "#2c3a4d",
    text: "#e6eef6",
    subtext: "#9fb0c2",
    faint: "#64748a",
    accent: "#7cc4ff",
    onAccent: "#04121f",
    signal: "#b29cff",
    error: "#ff6b7a",
    warn: "#f2c86b",
    ok: "#7fd6a4"
  })

  readonly property var moss: ({
    bg: "#0f120e",
    surface: "#171b15",
    raised: "#1f251c",
    overlay: "#283024",
    outline: "#323b2d",
    text: "#e9ecdf",
    subtext: "#a7ad99",
    faint: "#6b715f",
    accent: "#a8d46f",
    onAccent: "#11190a",
    signal: "#e0b458",
    error: "#ef6a5a",
    warn: "#e8c35a",
    ok: "#8fd18a"
  })

  readonly property var dawn: ({
    bg: "#f7f3ee",
    surface: "#efe9e2",
    raised: "#e6dfd6",
    overlay: "#dcd3c8",
    outline: "#d2c7ba",
    text: "#2a2521",
    subtext: "#5e554d",
    faint: "#8d837a",
    accent: "#d9622b",
    onAccent: "#fff7f0",
    signal: "#1f8f7a",
    error: "#c8364a",
    warn: "#b98100",
    ok: "#3f8f3a"
  })

  readonly property var own: ({ ember: ember, frost: frost, moss: moss, dawn: dawn })
  readonly property var palette: own[Config.palette] || ember

  property var omarchy: ({})
  readonly property bool followOmarchy: Config.palette === "classic" && omarchy.background !== undefined

  function c(key) {
    if (!root.followOmarchy)
      return root.palette[key];
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
    return map[key] || root.palette[key];
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

  // Style. `floating` styles keep the bar off the screen edge by `barInset`.
  readonly property string style: Config.style
  readonly property bool floating: style !== "ember"
  readonly property bool barAtBottom: style === "dock"
  readonly property int barInset: floating ? 10 : 0
  // How far from its screen edge the bar's far side is (windows start there).
  readonly property int barReach: barHeight + barInset

  // Geometry. In the ember style the bar is flush with the top edge and
  // flows into the screen through two concave corners; drawers grow out of
  // it the same way.
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

  // Omarchy's colors.toml: flat `key = "value"` lines.
  FileView {
    path: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
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
