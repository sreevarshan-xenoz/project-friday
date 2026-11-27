using System.Windows;
using FridayTray.Core;
using Wpf.Ui.Appearance;
using WpfColor = System.Windows.Media.Color;

namespace FridayTray.Fluent;

/// <summary>
/// The WPF side of the tray: the windows are WPF (animation, glass, Fluent
/// controls through WPF-UI) while the tray icon, hotkeys and message loop stay
/// WinForms. WinForms' loop pumps the WPF windows' messages too; this sets up
/// the shared resources once and shows a window so it gets keyboard input.
/// </summary>
internal static class FluentApp
{
    public static void Init()
    {
        if (System.Windows.Application.Current is not null)
            return;
        var app = new System.Windows.Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        app.Resources.MergedDictionaries.Add(new Wpf.Ui.Markup.ThemesDictionary { Theme = ApplicationTheme.Dark });
        app.Resources.MergedDictionaries.Add(new Wpf.Ui.Markup.ControlsDictionary());

        ApplyPalette(ChosenPalette());
    }

    /// <summary>The palette friday.env names (FRIDAY_PALETTE); Core when it
    /// names none or can't be read.</summary>
    public static string ChosenPalette()
    {
        try { return AppSettings.PaletteName(EnvFile.Load(Paths.ForCurrentUser().EnvFile)); }
        catch (Exception) { return "core"; }
    }

    private static ResourceDictionary? _palette, _ember;

    /// <summary>Puts a palette in place: its colours, then Ember.xaml's
    /// brushes built from them. Windows opened after this use it; open ones
    /// keep theirs (their brushes were looked up when they opened).</summary>
    public static void ApplyPalette(string name)
    {
        var app = System.Windows.Application.Current;
        if (app is null)
            return;
        var palette = AppSettings.PaletteColours(name);
        if (_palette is not null) app.Resources.MergedDictionaries.Remove(_palette);
        if (_ember is not null) app.Resources.MergedDictionaries.Remove(_ember);
        _palette = BuildPaletteDictionary(palette);
        app.Resources.MergedDictionaries.Add(_palette);
        _ember = new ResourceDictionary
        {
            Source = new Uri("pack://application:,,,/friday-tray;component/Fluent/Ember.xaml"),
        };
        app.Resources.MergedDictionaries.Add(_ember);
        ApplicationAccentColorManager.Apply(ParseColor(palette.Accent), ApplicationTheme.Dark);
    }

    private static ResourceDictionary BuildPaletteDictionary(PaletteColors palette)
    {
        var dict = new ResourceDictionary();
        var bg = ParseColor(palette.Bg);
        var accent = ParseColor(palette.Accent);

        dict["InkColor"] = bg;
        dict["GlassColor"] = WpfColor.FromArgb(0xD8, bg.R, bg.G, bg.B);
        dict["TextColor"] = ParseColor(palette.Text);
        dict["SubtextColor"] = ParseColor(palette.Subtext);
        dict["FaintColor"] = ParseColor(palette.Faint);
        dict["EmberColor"] = accent;
        dict["EmberHiColor"] = Lighten(accent, 0.20);
        dict["SignalColor"] = ParseColor(palette.Signal);
        dict["WarnColor"] = ParseColor(palette.Warn);
        dict["ErrorColor"] = ParseColor(palette.Error);

        var washStart = WpfColor.FromArgb(0x1F, accent.R, accent.G, accent.B);
        var washEnd = WpfColor.FromArgb(0x00, accent.R, accent.G, accent.B);
        dict["EmberWashStartColor"] = washStart;
        dict["EmberWashEndColor"] = washEnd;

        return dict;
    }

    private static WpfColor Lighten(WpfColor c, double amount) =>
        WpfColor.FromArgb(
            c.A,
            (byte)Math.Clamp((int)Math.Round(c.R + (255 - c.R) * amount), 0, 255),
            (byte)Math.Clamp((int)Math.Round(c.G + (255 - c.G) * amount), 0, 255),
            (byte)Math.Clamp((int)Math.Round(c.B + (255 - c.B) * amount), 0, 255));

    private static WpfColor ParseColor(string hex)
    {
        hex = hex.TrimStart('#');
        if (hex.Length == 6)
        {
            var r = Convert.ToByte(hex[..2], 16);
            var g = Convert.ToByte(hex[2..4], 16);
            var b = Convert.ToByte(hex[4..6], 16);
            return WpfColor.FromRgb(r, g, b);
        }
        if (hex.Length == 8)
        {
            var a = Convert.ToByte(hex[..2], 16);
            var r = Convert.ToByte(hex[2..4], 16);
            var g = Convert.ToByte(hex[4..6], 16);
            var b = Convert.ToByte(hex[6..8], 16);
            return WpfColor.FromArgb(a, r, g, b);
        }
        return WpfColor.FromRgb(0, 0, 0);
    }

    /// <summary>Shows a modeless window from the WinForms loop, with working keyboard input.</summary>
    public static void Show(Window window)
    {
        System.Windows.Forms.Integration.ElementHost.EnableModelessKeyboardInterop(window);
        window.Show();
    }
}
