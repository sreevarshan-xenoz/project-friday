using System.Drawing.Drawing2D;
using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// The tray icon (FRIDAY's mark, for each daemon state) and the few colours the
/// WinForms parts still draw with; the windows take theirs from Fluent/Ember.xaml.
/// </summary>
internal static class Theme
{
    private static readonly PaletteColors Palette = AppSettings.PaletteColours(
        AppSettings.PaletteName(EnvFile.Load(Paths.ForCurrentUser().EnvFile)));

    public static readonly Color Faint = ColorTranslator.FromHtml(Palette.Faint);
    public static readonly Color Accent = ColorTranslator.FromHtml(Palette.Accent);
    public static readonly Color Warn = ColorTranslator.FromHtml(Palette.Warn);

    // FRIDAY's mark (the aperture F) in the logo's colours, whatever the
    // windows' palette: obsidian disc, silver blades, platinum F, core red.
    private static readonly Color Obsidian = ColorTranslator.FromHtml("#111318");
    private static readonly Color Silver = ColorTranslator.FromHtml("#B8BBC3");
    private static readonly Color Platinum = ColorTranslator.FromHtml("#DDE0E6");
    private static readonly Color CoreRed = ColorTranslator.FromHtml("#D42D51");

    // The mark's shapes on a 100 x 100 box (six blades round an open centre,
    // a slanted F), the same drawing as FRIDAY Shell's Orb.qml.
    private static readonly PointF[][] Blades =
    {
        new PointF[] { new(50.0f, 6.0f), new(55.1f, 6.3f), new(60.1f, 7.2f), new(65.0f, 8.7f), new(69.7f, 10.7f), new(74.2f, 13.2f), new(78.3f, 16.3f), new(78.8f, 22.0f), new(78.7f, 27.4f), new(78.1f, 32.5f), new(76.9f, 37.2f), new(75.2f, 41.6f), new(73.0f, 45.7f), new(70.1f, 49.5f), new(66.7f, 53.0f), new(67.5f, 47.9f), new(67.4f, 42.7f), new(66.5f, 37.2f), new(64.8f, 31.4f), new(62.3f, 25.4f), new(59.0f, 19.2f), new(54.9f, 12.7f) },
        new PointF[] { new(88.1f, 28.0f), new(90.4f, 32.6f), new(92.2f, 37.4f), new(93.3f, 42.4f), new(93.9f, 47.4f), new(93.9f, 52.6f), new(93.3f, 57.6f), new(88.6f, 60.9f), new(83.9f, 63.6f), new(79.2f, 65.6f), new(74.5f, 66.9f), new(69.9f, 67.7f), new(65.2f, 67.7f), new(60.5f, 67.2f), new(55.8f, 66.0f), new(60.5f, 64.1f), new(65.0f, 61.4f), new(69.4f, 57.9f), new(73.5f, 53.6f), new(77.4f, 48.4f), new(81.2f, 42.4f), new(84.7f, 35.6f) },
        new PointF[] { new(88.1f, 72.0f), new(85.3f, 76.3f), new(82.0f, 80.2f), new(78.3f, 83.7f), new(74.2f, 86.8f), new(69.7f, 89.3f), new(65.0f, 91.3f), new(59.9f, 88.9f), new(55.2f, 86.2f), new(51.1f, 83.1f), new(47.6f, 79.7f), new(44.6f, 76.0f), new(42.2f, 72.0f), new(40.4f, 67.7f), new(39.1f, 63.0f), new(43.1f, 66.2f), new(47.6f, 68.7f), new(52.9f, 70.7f), new(58.7f, 72.1f), new(65.1f, 73.0f), new(72.2f, 73.2f), new(79.8f, 72.9f) },
        new PointF[] { new(50.0f, 94.0f), new(44.9f, 93.7f), new(39.9f, 92.8f), new(35.0f, 91.3f), new(30.3f, 89.3f), new(25.8f, 86.8f), new(21.7f, 83.7f), new(21.2f, 78.0f), new(21.3f, 72.6f), new(21.9f, 67.5f), new(23.1f, 62.8f), new(24.8f, 58.4f), new(27.0f, 54.3f), new(29.9f, 50.5f), new(33.3f, 47.0f), new(32.5f, 52.1f), new(32.6f, 57.3f), new(33.5f, 62.8f), new(35.2f, 68.6f), new(37.7f, 74.6f), new(41.0f, 80.8f), new(45.1f, 87.3f) },
        new PointF[] { new(11.9f, 72.0f), new(9.6f, 67.4f), new(7.8f, 62.6f), new(6.7f, 57.6f), new(6.1f, 52.6f), new(6.1f, 47.4f), new(6.7f, 42.4f), new(11.4f, 39.1f), new(16.1f, 36.4f), new(20.8f, 34.4f), new(25.5f, 33.1f), new(30.1f, 32.3f), new(34.8f, 32.3f), new(39.5f, 32.8f), new(44.2f, 34.0f), new(39.5f, 35.9f), new(35.0f, 38.6f), new(30.6f, 42.1f), new(26.5f, 46.4f), new(22.6f, 51.6f), new(18.8f, 57.6f), new(15.3f, 64.4f) },
        new PointF[] { new(11.9f, 28.0f), new(14.7f, 23.7f), new(18.0f, 19.8f), new(21.7f, 16.3f), new(25.8f, 13.2f), new(30.3f, 10.7f), new(35.0f, 8.7f), new(40.1f, 11.1f), new(44.8f, 13.8f), new(48.9f, 16.9f), new(52.4f, 20.3f), new(55.4f, 24.0f), new(57.8f, 28.0f), new(59.6f, 32.3f), new(60.9f, 37.0f), new(56.9f, 33.8f), new(52.4f, 31.3f), new(47.1f, 29.3f), new(41.3f, 27.9f), new(34.9f, 27.0f), new(27.8f, 26.8f), new(20.2f, 27.1f) }
    };

    private static readonly PointF[] Letter = new PointF[] { new(44.4f, 24.0f), new(82.4f, 24.0f), new(71.7f, 33.0f), new(52.7f, 33.0f), new(48.4f, 44.0f), new(66.4f, 44.0f), new(57.0f, 52.0f), new(45.0f, 52.0f), new(36.4f, 74.0f), new(24.0f, 82.0f) };

    /// <summary>The mark for each daemon state: ready = silver blades and
    /// the F; working = dim blades and a core-red dot; stopped = amber blades;
    /// offline = the mark dimmed.</summary>
    public static Icon Orb(string mode, int size = 32)
    {
        using var bmp = new Bitmap(size, size);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);
            using (var disc = new SolidBrush(Obsidian))
                g.FillEllipse(disc, 0, 0, size - 1, size - 1);
            g.ScaleTransform(size / 100f * 0.9f, size / 100f * 0.9f);
            g.TranslateTransform(5.5f, 5.5f);
            var blade = mode switch
            {
                "stopped" => Warn,
                "offline" => Color.FromArgb(110, Faint),
                "working" => Color.FromArgb(90, Silver),
                _ => Silver,
            };
            using (var b = new SolidBrush(blade))
                foreach (var poly in Blades) g.FillPolygon(b, poly);
            if (mode == "working")
            {
                using var dot = new SolidBrush(CoreRed);
                g.FillEllipse(dot, 36, 36, 28, 28);
            }
            else
            {
                using (var band = new Pen(Obsidian, 12) { LineJoin = LineJoin.Round })
                    g.DrawPolygon(band, Letter);
                using var letter = new SolidBrush(mode == "offline" ? Faint : Platinum);
                g.FillPolygon(letter, Letter);
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
