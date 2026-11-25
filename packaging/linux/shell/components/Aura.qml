import QtQuick
import qs.common

// FRIDAY's presence while it listens or thinks: layered ribbons of light that
// flow like a voice, pinched to nothing at both ends and glowing where they
// cross (colours add up). Drawn on a 2D canvas so it looks the same on the
// software renderer used in VMs.
//   listening  slow, wide swells, breathing like someone speaking
//   speaking   steady, lively swells while FRIDAY talks
//   working    quick, tight, shimmering
// Colours come in as properties so the component stands alone.
Item {
  id: root

  property real size: 40
  property string mode: "working"
  // Still (motion off): drawn once for each change instead of every frame.
  onModeChanged: if (!Theme.motion) canvas.requestPaint()
  onEnergyChanged: if (!Theme.motion) canvas.requestPaint()
  property color primary: "#ff8f4c"
  property color secondary: "#5fd3bc"
  property color tertiary: "#b48cff"

  readonly property bool fast: mode === "working"

  implicitWidth: size * 3
  implicitHeight: size

  // Time in seconds, advanced by the frame timer; the energy eases between modes.
  property real t: 0
  property real energy: fast ? 1.0 : mode === "speaking" ? 0.8 : 0.55
  Behavior on energy { NumberAnimation { duration: 600; easing.type: Easing.InOutSine } }

  Timer {
    interval: 33
    repeat: true
    running: root.visible && Theme.motion
    onTriggered: { root.t += 0.033 * (0.6 + root.energy); canvas.requestPaint(); }
  }

  Canvas {
    id: canvas
    anchors.fill: parent
    renderTarget: Canvas.Image

    onPaint: {
      var ctx = getContext("2d");
      var w = width, h = height, mid = h / 2;
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      // A slow swell, as if a voice rose and fell.
      var swell = 0.55 + 0.45 * Math.sin(root.t * (root.fast ? 5.0 : 2.2));
      var amp = h * 0.42 * (0.35 + 0.65 * swell) * (0.6 + 0.4 * root.energy);

      var layers = [
        { c: root.primary,   f: 1.3, s: 1.0,  p: 0.0 },
        { c: root.secondary, f: 1.9, s: -1.3, p: 1.7 },
        { c: root.tertiary,  f: 2.6, s: 1.7,  p: 3.1 },
        { c: root.primary,   f: 3.4, s: -2.1, p: 4.6 }
      ];

      for (var i = 0; i < layers.length; i++) {
        var L = layers[i];
        var a = amp * (1.0 - i * 0.16);
        var steps = 48;
        ctx.beginPath();
        for (var k = 0; k <= steps; k++) {
          var u = k / steps;
          var x = u * w;
          var taper = Math.pow(Math.sin(Math.PI * u), 2);
          var y = mid + Math.sin(u * Math.PI * 2 * L.f + root.t * L.s * (0.8 + root.energy) + L.p) * a * taper;
          if (k === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        }
        for (var k2 = steps; k2 >= 0; k2--) {
          var u2 = k2 / steps;
          var x2 = u2 * w;
          var taper2 = Math.pow(Math.sin(Math.PI * u2), 2);
          var y2 = mid + Math.sin(u2 * Math.PI * 2 * L.f + root.t * L.s * (0.8 + root.energy) + L.p + 0.9) * a * taper2 * 0.8;
          ctx.lineTo(x2, y2);
        }
        ctx.closePath();
        ctx.fillStyle = Qt.rgba(L.c.r, L.c.g, L.c.b, 0.4);
        ctx.fill();
        ctx.lineWidth = 1.4;
        ctx.strokeStyle = Qt.rgba(L.c.r, L.c.g, L.c.b, 0.85);
        ctx.stroke();
      }

      // A soft elliptical glow at the centre; it fades to nothing inside the frame.
      ctx.save();
      ctx.translate(w / 2, mid);
      ctx.scale(2.4, 1);
      var g = ctx.createRadialGradient(0, 0, 0, 0, 0, h * 0.5);
      g.addColorStop(0, Qt.rgba(1, 0.93, 0.85, 0.6));
      g.addColorStop(1, Qt.rgba(1, 0.93, 0.85, 0));
      ctx.fillStyle = g;
      ctx.fillRect(-h * 0.5, -h * 0.5, h, h);
      ctx.restore();
    }
  }
}
