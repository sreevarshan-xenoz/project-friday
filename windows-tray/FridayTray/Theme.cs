using System.Drawing.Drawing2D;

namespace FridayTray;

/// <summary>
/// The tray icon (the orb, for each daemon state) and the few colours the
/// WinForms parts still draw with; the windows take theirs from Fluent/Ember.xaml.
/// </summary>
internal static class Theme
{
    public static readonly Color Faint = ColorTranslator.FromHtml("#6c6862");
    public static readonly Color Accent = ColorTranslator.FromHtml("#ff8f4c");
    public static readonly Color Signal = ColorTranslator.FromHtml("#5fd3bc");
    public static readonly Color Warn = ColorTranslator.FromHtml("#f5c451");

    /// <summary>The orb: a ring with a core. Ready = ember core, working = ember
    /// core with a signal arc, stopped = amber core, offline = an empty ring.</summary>
    public static Icon Orb(string mode, int size = 32)
    {
        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);
            var w = size / 12f;
            var ring = new RectangleF(w, w, size - 2 * w, size - 2 * w);
            using (var pen = new Pen(mode == "offline" ? Faint : Color.FromArgb(150, Accent), w))
                g.DrawEllipse(pen, ring);
            if (mode == "working")
            {
                using var arc = new Pen(Signal, w * 1.3f) { StartCap = LineCap.Round, EndCap = LineCap.Round };
                g.DrawArc(arc, ring, -90, 120);
            }
            if (mode != "offline")
            {
                var r = size * 0.22f;
                using var core = new SolidBrush(mode == "stopped" ? Warn : Accent);
                g.FillEllipse(core, size / 2f - r, size / 2f - r, 2 * r, 2 * r);
            }
        }
        var hIcon = bmp.GetHicon();
        try
        {
            using var icon = Icon.FromHandle(hIcon);
            return (Icon)icon.Clone();
        }
        finally
        {
            Native.DestroyIcon(hIcon);
        }
    }
}
