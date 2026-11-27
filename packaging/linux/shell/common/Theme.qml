pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// FRIDAY Shell's look: a palette, a style, one type scale, one motion curve.
//
// Palettes (shell.json "palette"): FRIDAY's own, each with an accent for
// FRIDAY and anything it is doing and a signal colour for system state:
//   core   obsidian, silver and platinum text, core-red accent, steel-blue
//          signal: the colours of FRIDAY's logo (the default)
//   ember  warm near-black, ember accent, teal signal
//   polar  Nord's arctic blue-greys, frost-cyan accent, aurora purple signal
//   frost  cool slate, ice-blue accent, lilac signal
//   moss   dark olive, lichen-green accent, amber signal
//   dawn   light: warm paper, rust accent, deep-teal signal
//   contrast  high contrast: black, white text and edges, yellow accent,
//          no see-through glass
// or "classic": follow the active desktop theme ($XDG_STATE_HOME/omarchy/
// current/theme/colors.toml, where omarchy-theme-set links it), live.
//
// Styles (shell.json "style") change where the bar sits and its shape:
//   ember   flush with the top edge, flowing into the screen through
//           concave corners; drawers grow out of it
//   lean    the ember bar with windows kept plain: thin borders, small gaps,
//           no blur (FRIDAY OS's default)
//   island  three floating pills along the top; drawers are floating cards
//   dock    one floating bar along the bottom; drawers rise from it
//   rail    one slim column down the left edge; drawers open beside it
//   orbit   two floating pills, apps and workspaces at the top left, FRIDAY
//           and the status at the top right; the middle stays clear
//   glaze   one floating glass bar along the top with a lit rim; a band of
//           FRIDAY's colours sweeps along it while it works
//   statusline  one slim solid line along the bottom, in terminal-style
//           segments; no glass, no motion in it
//   keystone  no bar across the screen: one notch hangs from the middle of
//           the top edge with the workspaces, FRIDAY and the status in it
Singleton {
  id: root

  readonly property var core: ({
    bg: "#111318",
    surface: "#181b21",
    raised: "#20232a",
    overlay: "#2a2e36",
    outline: "#343842",
    text: "#dde0e6",
    subtext: "#b8bbc3",
    faint: "#9498a2",
    accent: "#d42d51",
    onAccent: "#fff4f6",
    signal: "#8ea4c8",
    error: "#ff5d6c",
    warn: "#e8b04f",
    ok: "#6fbf8a"
  })

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

  readonly property var polar: ({
    bg: "#2e3440",
    surface: "#3b4252",
    raised: "#434c5e",
    overlay: "#4c566a",
    outline: "#4c566a",
    text: "#eceff4",
    subtext: "#d8dee9",
    faint: "#7b88a1",
    accent: "#88c0d0",
    onAccent: "#242933",
    signal: "#b48ead",
    error: "#bf616a",
    warn: "#ebcb8b",
    ok: "#a3be8c"
  })

  readonly property var contrast: ({
    bg: "#000000",
    surface: "#0a0a0a",
    raised: "#1c1c1c",
    overlay: "#333333",
    outline: "#ffffff",
    text: "#ffffff",
    subtext: "#f2f2f2",
    faint: "#cccccc",
    accent: "#ffd60a",
    onAccent: "#000000",
    signal: "#5ac8fa",
    error: "#ff6b6b",
    warn: "#ffb000",
    ok: "#5ee36a"
  })

  readonly property var own: ({ core: core, ember: ember, polar: polar, frost: frost, moss: moss, dawn: dawn, contrast: contrast })
  readonly property bool highContrast: Config.palette === "contrast"
  readonly property var palette: own[Config.palette] || core

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
  // The wallpaper's own accent (`friday wallpaper` works it out; switch:
  // wallpaper-accent), else the palette's. Text on it is dark or light by
  // its brightness.
  readonly property string wallpaperAccent: {
    const v = Config.pick("wallpaperAccent", "");
    return Config.featureOn("wallpaper-accent") && Config.palette !== "classic" && !highContrast && /^#[0-9a-fA-F]{6}$/.test(v) ? v : "";
  }
  readonly property color accent: wallpaperAccent || c("accent")
  readonly property color onAccent: wallpaperAccent
    ? (0.2126 * accent.r + 0.7152 * accent.g + 0.0722 * accent.b > 0.5 ? "#111318" : "#fff4f6")
    : c("onAccent")
  readonly property color signal: c("signal")
  readonly property color error: c("error")
  readonly property color warn: c("warn")
  readonly property color ok: c("ok")

  function alpha(col, a) { return Qt.rgba(col.r, col.g, col.b, a); }

  // Frosted glass: the bar, drawers, answer card, notifications and OSD let
  // the desktop show through, blurred where the compositor can (Hyprland:
  // the layer rules in hypr-look.lua). Opaque enough to read without blur.
  readonly property bool light: bg.hslLightness > 0.5
  readonly property color glass: highContrast ? bg : alpha(bg, light ? 0.8 : 0.72)
  readonly property color glassWell: alpha(text, highContrast ? 0.14 : light ? 0.07 : 0.06)
  readonly property color glassEdge: highContrast ? text : alpha(text, light ? 0.16 : 0.11)

  // Type: Noto Sans for words, JetBrains Mono for numbers and glyphs.
  // FRIDAY's type (packaging/linux/fonts, OFL): Space Grotesk for text,
  // Michroma for the FRIDAY wordmark, JetBrains Mono for numbers and icons.
  readonly property string sans: "Space Grotesk"
  readonly property string display: "Michroma"
  readonly property string mono: "JetBrainsMono Nerd Font Propo"
  readonly property int fontS: 12
  readonly property int fontM: 13
  readonly property int fontL: 16
  readonly property int fontXL: 22

  // Style. `floating` styles keep the bar off the screen edge by `barInset`.
  readonly property string style: Config.style
  readonly property bool floating: style === "island" || style === "dock" || style === "orbit" || style === "glaze"
  // The rail: a column down the left edge instead of a bar along the top.
  readonly property bool rail: style === "rail"
  readonly property int railWidth: 46
  // How far from the left edge windows and desktop things start.
  readonly property int leftReach: rail ? railWidth : 0
  // The keystone: everything in one notch at the top's middle.
  readonly property bool keystone: style === "keystone"
  // The statusline: one slim solid line along the bottom, in segments.
  readonly property bool statusline: style === "statusline"
  readonly property bool barAtBottom: style === "dock" || statusline
  readonly property int barInset: floating ? 10 : 0
  // How far from its screen edge the bar's far side is (windows start there).
  readonly property int barReach: rail ? 0 : barHeight + barInset

  // Geometry. In the ember style the bar is flush with the top edge and
  // flows into the screen through two concave corners; drawers grow out of
  // it the same way.
  readonly property int barHeight: statusline ? 24 : 36
  readonly property int fillet: 16
  readonly property int radius: 20
  readonly property int radiusS: 10
  readonly property int gap: 8
  readonly property int pad: 16

  // Motion: one decelerating curve, three durations. With motion switched
  // off (friday feature motion off) everything changes at once and nothing
  // pulses or loops.
  readonly property bool motion: Config.featureOn("motion")
  readonly property int fast: motion ? 140 : 0
  readonly property int medium: motion ? 260 : 0
  readonly property int slow: motion ? 420 : 0
  readonly property int easing: Easing.OutCubic
  // Things that appear spring in: past their mark a little, then settle.
  readonly property int spring: motion ? Easing.OutBack : Easing.OutCubic

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
