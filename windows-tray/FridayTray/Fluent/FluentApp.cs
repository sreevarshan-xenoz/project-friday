using System.Windows;
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
        app.Resources.MergedDictionaries.Add(new ResourceDictionary
        {
            Source = new Uri("pack://application:,,,/friday-tray;component/Fluent/Ember.xaml"),
        });
        ApplicationAccentColorManager.Apply(WpfColor.FromRgb(0xFF, 0x8F, 0x4C), ApplicationTheme.Dark);
    }

    /// <summary>Shows a modeless window from the WinForms loop, with working keyboard input.</summary>
    public static void Show(Window window)
    {
        System.Windows.Forms.Integration.ElementHost.EnableModelessKeyboardInterop(window);
        window.Show();
    }
}
