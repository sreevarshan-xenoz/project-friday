using System.Drawing.Drawing2D;

namespace FridayTray;

/// <summary>
/// FRIDAY's look on Windows, the same "Ember on Ink" palette as FRIDAY Shell
/// on Linux, and the orb drawn as the tray icon for each daemon state.
/// </summary>
internal static class Theme
{
    public static readonly Color Bg = ColorTranslator.FromHtml("#0d0f13");
    public static readonly Color Surface = ColorTranslator.FromHtml("#16191f");
    public static readonly Color Raised = ColorTranslator.FromHtml("#1e222a");
    public static readonly Color Outline = ColorTranslator.FromHtml("#323844");
    public static readonly Color Text = ColorTranslator.FromHtml("#ece8e2");
    public static readonly Color Subtext = ColorTranslator.FromHtml("#a39e96");
    public static readonly Color Faint = ColorTranslator.FromHtml("#6c6862");
    public static readonly Color Accent = ColorTranslator.FromHtml("#ff8f4c");
    public static readonly Color OnAccent = ColorTranslator.FromHtml("#1d0f05");
    public static readonly Color Signal = ColorTranslator.FromHtml("#5fd3bc");
    public static readonly Color Warn = ColorTranslator.FromHtml("#f5c451");
    public static readonly Color Error = ColorTranslator.FromHtml("#ff5d6c");

    public static readonly Font Body = new("Segoe UI", 10.5f);
    public static readonly Font Small = new("Segoe UI", 9f);
    public static readonly Font Title = new("Segoe UI Semibold", 11f);
    public static readonly Font Input = new("Segoe UI", 13f);
    public static readonly Font Monospace = new(FontFamily.GenericMonospace, 10f);

    [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);

    /// <summary>A dark title bar to match the dark window (Windows 10 2004 and
    /// later; older ones keep their light bar).</summary>
    public static void DarkTitleBar(Form form) => form.HandleCreated += (_, _) =>
    {
        var on = 1;
        _ = DwmSetWindowAttribute(form.Handle, 20, ref on, sizeof(int)); // DWMWA_USE_IMMERSIVE_DARK_MODE
    };

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

    /// <summary>A flat button in the theme.</summary>
    public static Button Button(string text, bool primary = false)
    {
        var b = new Button
        {
            Text = text,
            AutoSize = true,
            FlatStyle = FlatStyle.Flat,
            BackColor = primary ? Accent : Raised,
            ForeColor = primary ? OnAccent : Text,
            Font = Small,
            Padding = new Padding(10, 4, 10, 4),
            Cursor = Cursors.Hand,
        };
        // A visible edge and a hover colour, so a button never looks disabled
        // against the dark surface.
        b.FlatAppearance.BorderSize = primary ? 0 : 1;
        b.FlatAppearance.BorderColor = Outline;
        b.FlatAppearance.MouseOverBackColor = primary ? ControlPaint.Light(Accent, 0.2f) : ColorTranslator.FromHtml("#2a2f39");
        b.FlatAppearance.MouseDownBackColor = primary ? ControlPaint.Dark(Accent, 0.1f) : Surface;
        return b;
    }
}
