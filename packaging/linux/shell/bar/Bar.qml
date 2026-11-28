import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import qs.common
import qs.components
import qs.services

// The bar. Left: launcher and workspaces. Centre: FRIDAY and the clock.
// Right: tray, media, the status pill and power. Where it sits and its shape
// follow the style (Theme.style):
//   ember   flush with the top edge, flowing into the screen through two
//           concave corners
//   island  the three groups as separate floating pills along the top
//   dock    one floating bar along the bottom
//   keystone  one notch hanging from the middle of the top edge, the groups
//           pulled in around FRIDAY's island (the rest of the edge is free)
//   neon    slim and square-cut along the top, a hairline of accent under it
//   monolith  one solid square slab along the top, "//" between its parts
PanelWindow {
  id: bar

  required property ShellScreen modelData
  screen: modelData

  // Pill styles: island (three pills) and orbit (two, the middle left clear).
  readonly property bool island: Theme.style === "island" || orbit
  readonly property bool orbit: Theme.style === "orbit"
  // Glaze: one floating glass bar along the top, with a lit rim.
  readonly property bool glaze: Theme.style === "glaze"
  // Blade: flush, a hairline under it that follows the active workspace in
  // FRIDAY's colour, and volume and brightness that slide open on hover.
  readonly property bool blade: Theme.style === "blade"
  // Monolith: a solid slab (no glass), "//" between its parts.
  readonly property bool monolith: Theme.monolith

  // Auto-hide (Look > Auto-hide bar; shell.json "barAutoHide"; off unless
  // turned on): the bar slides off its edge and windows get the space; the
  // pointer at the edge, an open drawer, or FRIDAY listening or working brings
  // it back, and it goes half a second after the pointer leaves. An idea from
  // end-4/dots-hyprland (see CREDITS.md).
  readonly property bool autoHide: Config.barAutoHide
  readonly property bool wanted: barHover.hovered || Shell.menuOpen
    || (Shell.drawer !== "" && Shell.screen === bar.modelData.name)
    || Answer.state === "listening" || Friday.working
  property bool held: false
  onWantedChanged: if (wanted) { held = true; leave.stop(); } else leave.restart()
  Timer { id: leave; interval: 500; onTriggered: bar.held = false }
  readonly property bool hidden: autoHide && !wanted && !held
  // How far the bar has slid off its edge (past the corners under it too).
  property real slide: hidden ? Theme.barReach + Theme.fillet + 6 : 0
  Behavior on slide { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
  HoverHandler { id: barHover }

  anchors { top: !Theme.barAtBottom; bottom: Theme.barAtBottom; left: true; right: true }
  // Room for the island to open below the bar (it's masked, so clicks pass).
  implicitHeight: (Theme.floating ? Theme.barReach : Theme.barHeight + Theme.fillet) + 40
  // Auto-hide: no space kept for it (ExclusionMode.Ignore). The zone is only
  // set while auto-hide is off: setting it puts the mode back to Normal, so a
  // style switch would otherwise keep space for a hidden bar.
  exclusionMode: autoHide ? ExclusionMode.Ignore : ExclusionMode.Normal
  Binding {
    target: bar
    property: "exclusiveZone"
    when: !bar.autoHide
    value: Theme.floating ? Theme.barReach : Theme.barHeight
    restoreMode: Binding.RestoreNone
  }
  color: "transparent"
  WlrLayershell.namespace: "friday-bar"
  WlrLayershell.layer: WlrLayer.Top

  // Only the bar takes input: the strip, or in the island style just its
  // pills, so clicks between them reach the windows below.
  // With auto-hide, the edge strip always takes the pointer too, so the bar
  // doesn't lose it while sliding back in.
  mask: Region {
    item: bar.autoHide ? edgeStrip : null
    Region { item: bar.hidden || bar.island ? null : Theme.keystone ? notch : strip }
    Region { item: !bar.hidden && bar.island ? leftPill : null }
    Region { item: bar.hidden ? null : centre.capsule }
    Region { item: !bar.hidden && bar.island ? rightPill : null }
  }

  // Auto-hide: the strip along the edge that brings the bar back.
  Item {
    id: edgeStrip
    anchors { left: parent.left; right: parent.right }
    y: Theme.barAtBottom ? parent.height - height : 0
    height: 3
  }

  Rectangle {
    id: strip
    // Placed by y rather than a top or bottom anchor: a style switch that
    // swaps the anchors can leave the old one set and stretch the strip.
    anchors { left: parent.left; right: parent.right; margins: Theme.barInset }
    y: Theme.barAtBottom ? parent.height - height - Theme.barInset + bar.slide : Theme.barInset - bar.slide
    height: Theme.barHeight
    radius: Theme.floating ? height / 2 : 0
    color: bar.island || Theme.keystone || !Config.barBackground ? "transparent" : bar.monolith ? Theme.surface : Theme.glass
    border.width: (Theme.style === "dock" || bar.glaze) && Config.barBackground ? 1 : 0
    border.color: Theme.glassEdge

    // Glaze: a fine light along the top rim, a soft shade along the bottom
    // one, and while FRIDAY works a band of its colours sweeps along the rim.
    Rectangle {
      visible: bar.glaze && Config.barBackground
      anchors { top: parent.top; topMargin: 1; horizontalCenter: parent.horizontalCenter }
      width: parent.width - 2 * parent.radius
      height: 1
      gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: "transparent" }
        GradientStop { position: 0.5; color: Theme.alpha("#ffffff", Theme.light ? 0.7 : 0.28) }
        GradientStop { position: 1; color: "transparent" }
      }
    }
    Rectangle {
      visible: bar.glaze && Config.barBackground
      anchors { bottom: parent.bottom; bottomMargin: 1; horizontalCenter: parent.horizontalCenter }
      width: parent.width - 2 * parent.radius
      height: 1
      color: Theme.alpha("#000000", Theme.light ? 0.08 : 0.3)
    }
    Item {
      id: sweepTrack
      visible: bar.glaze && Friday.working
      anchors { top: parent.top; left: parent.left; right: parent.right; leftMargin: parent.radius; rightMargin: parent.radius }
      height: 2
      clip: true
      Rectangle {
        id: sweep
        width: sweepTrack.width * 0.3
        height: 2
        radius: 1
        gradient: Gradient {
          orientation: Gradient.Horizontal
          GradientStop { position: 0; color: "transparent" }
          GradientStop { position: 0.4; color: Theme.accent }
          GradientStop { position: 0.7; color: Theme.signal }
          GradientStop { position: 1; color: "transparent" }
        }
        // Only while it shows, and not with motion off.
        NumberAnimation on x {
          running: sweepTrack.visible && Theme.motion
          from: -sweep.width; to: sweepTrack.width
          duration: 1600
          loops: Animation.Infinite
          easing.type: Easing.InOutSine
        }
      }
    }
  }
  // Keystone: the notch, flush with the top edge, round at the bottom, with
  // a small curve on each side where it meets the edge.
  Rectangle {
    id: notch
    visible: Theme.keystone
    x: leftGroup.x - 16
    width: rightGroup.x + rightGroup.width + 16 - x
    y: -radius - bar.slide
    height: Theme.barHeight + radius
    radius: 16
    color: Config.barBackground ? Theme.glass : "transparent"
    border.width: Config.barBackground ? 1 : 0
    border.color: Theme.glassEdge
  }
  // Corners: off with "barCorners" false (plain) or with no background.
  readonly property bool corners: Config.barCorners && Config.barBackground
  Corner { visible: Theme.keystone && bar.corners; size: 10; leftSide: false; x: notch.x - size; y: -bar.slide }
  Corner { visible: Theme.keystone && bar.corners; size: 10; leftSide: true; x: notch.x + notch.width; y: -bar.slide }

  Corner { visible: !Theme.floating && !Theme.keystone && !Theme.sharp && bar.corners; leftSide: true; anchors.top: strip.bottom; anchors.left: parent.left }
  Corner { visible: !Theme.floating && !Theme.keystone && !Theme.sharp && bar.corners; leftSide: false; anchors.top: strip.bottom; anchors.right: parent.right }

  // Blade: the hairline along the bar's lower edge, and the lit part of it
  // under the active workspace (the signal colour while FRIDAY works).
  Rectangle {
    visible: bar.blade
    x: strip.x; width: strip.width
    y: strip.y + strip.height - 1
    height: 1
    color: Theme.glassEdge
  }
  Rectangle {
    visible: bar.blade && workspaces.activeW > 0
    x: leftGroup.x + workspaces.x + workspaces.activeX - 4
    width: workspaces.activeW + 8
    y: strip.y + strip.height - 2
    height: 2
    radius: 1
    color: Friday.working ? Theme.signal : Theme.accent
    Behavior on x { NumberAnimation { duration: Theme.medium; easing.type: Theme.easing } }
    Behavior on color { ColorAnimation { duration: Theme.medium } }
  }

  // Neon: the hairline, brightest in the middle, gone at the edges.
  Rectangle {
    visible: Theme.style === "neon"
    anchors { left: strip.left; right: strip.right; top: strip.bottom }
    height: 1
    gradient: Gradient {
      orientation: Gradient.Horizontal
      GradientStop { position: 0; color: "transparent" }
      GradientStop { position: 0.5; color: Theme.alpha(Theme.accent, 0.55) }
      GradientStop { position: 1; color: "transparent" }
    }
  }

  // Monolith: a solid line along the slab's foot.
  Rectangle {
    visible: bar.monolith && Config.barBackground
    anchors { left: strip.left; right: strip.right; top: strip.bottom }
    height: 1
    color: Theme.outline
  }
  // Its "//" between the parts.
  component Slashes: Text {
    visible: bar.monolith
    text: "//"
    color: Theme.faint
    font.family: "monospace"
    font.pixelSize: Theme.fontS
    font.bold: true
  }

  // The island style's pills: one behind each group.
  component Pill: Rectangle {
    required property Item group
    // Where the pill starts, if left of its group (orbit: FRIDAY's island).
    property Item from: group
    visible: bar.island && Config.barBackground
    x: from.x - 10
    y: strip.y
    width: group.x + group.width - from.x + 20
    height: strip.height
    radius: Theme.round(height)
    color: Theme.glass
    border.width: 1
    border.color: Theme.glassEdge
  }
  Pill { id: leftPill; group: leftGroup }
  Pill { id: rightPill; group: rightGroup; from: bar.orbit ? centre : rightGroup }

  // Left.
  RowLayout {
    id: leftGroup
    // Keystone: just left of the island instead of at the screen's edge.
    anchors {
      left: Theme.keystone ? undefined : strip.left
      right: Theme.keystone ? centre.left : undefined
      leftMargin: 10
      rightMargin: 14
      verticalCenter: strip.verticalCenter
    }
    spacing: 10

    IconButton {
      tooltip: "Apps"
      icon: Icons.apps
      size: 26
      iconSize: 16
      active: Shell.isOpen("launcher", bar.modelData.name)
      onClicked: Shell.toggle("launcher", bar.modelData.name)
    }
    Workspaces { id: workspaces; screen: bar.modelData }
    Slashes {}
    WindowTitle { visible: !Theme.keystone && !bar.orbit; Layout.maximumWidth: Math.max(0, centre.x - strip.x - 260) }
  }

  // Centre: the aperture island (bar/Island.qml), centred on the bar but
  // never over the apps button and workspaces on a narrow screen. It drops
  // below the bar (or rises above the dock) when it opens.
  Island {
    id: centre
    screenName: bar.modelData.name
    growUp: Theme.barAtBottom
    y: strip.y + 4
    // Orbit: in the right pill, just before the status.
    x: bar.orbit ? Math.round(rightGroup.x - width - 14)
      : Theme.keystone ? Math.round(strip.x + strip.width / 2 - width / 2)
      : Math.round(Math.max(leftGroup.x + workspaces.x + workspaces.width + 24,
      strip.x + strip.width / 2 - width / 2))
  }

  // Right.
  RowLayout {
    id: rightGroup
    // Keystone: just right of the island.
    anchors {
      right: Theme.keystone ? undefined : strip.right
      left: Theme.keystone ? centre.right : undefined
      rightMargin: 10
      leftMargin: 14
      verticalCenter: strip.verticalCenter
    }
    spacing: 12

    Slashes {}
    ToolButtons {}
    ResourcesChip {}
    SlideControl {
      visible: bar.blade && Audio.ready
      icon: Audio.muted || Audio.volume <= 0.001 ? Icons.volumeOff : Icons.volumeHigh
      value: Audio.volume
      tooltip: "Volume"
      onMoved: v => Audio.setVolume(v)
    }
    SlideControl {
      visible: bar.blade && Brightness.ready
      icon: Icons.brightness
      value: Brightness.value
      tooltip: "Brightness"
      onMoved: v => Brightness.set(v)
    }
    Tray {}
    MediaChip {}
    StatusCluster {
      onClicked: Shell.toggle("control", bar.modelData.name)
    }
    IconButton {
      tooltip: "Power and session"
      icon: Icons.power
      size: 26
      iconSize: 15
      tint: Theme.error
      active: Shell.isOpen("session", bar.modelData.name)
      onClicked: Shell.toggle("session", bar.modelData.name)
    }
  }
}
