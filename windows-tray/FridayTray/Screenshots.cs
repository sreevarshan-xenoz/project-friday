using System.Drawing.Imaging;
using FridayTray.Core;
using FridayTray.Fluent;

namespace FridayTray;

/// <summary>
/// `friday-tray.exe --screenshots &lt;folder&gt;`: opens each window with sample
/// content, saves a picture of it, closes it, and exits. For looking at the
/// Windows side without clicking through it (CI runs it on a real desktop).
/// Nothing is asked, sent or changed: every button does nothing here.
/// </summary>
internal static class Screenshots
{
    public static int Run(Install install, string folder)
    {
        Directory.CreateDirectory(folder);
        Console.WriteLine("[screenshots] starting");
        ApplicationConfiguration.Initialize();
        FluentApp.Init();
        Console.WriteLine("[screenshots] WPF ready");
        var keys = new Dictionary<AskKind, string>
        {
            [AskKind.Plain] = "Win+Y", [AskKind.Selection] = "Win+Alt+Y", [AskKind.Screen] = "Win+Ctrl+Y",
        };
        var nothing = new SettingsActions(() => { }, () => { }, () => { }, () => { }, () => { }, () => { },
            () => { }, () => { }, _ => { }, () => { }, () => { }, () => { }, keys);

        foreach (var section in new[] { "Home", "General", "Privacy", "AI", "Voice", "Desktop control", "Tasks", "Shortcuts", "Health & updates", "About" })
            SaveWpf(new SettingsWindow(install, nothing, section), folder, "settings-" + section.Replace(" & ", "-").Replace(' ', '-').ToLowerInvariant());
        SaveWpf(new WelcomeWindow(keys, () => { }, () => { }, () => { }), folder, "welcome");
        SaveWpf(new AskWindow(AskKind.Plain, null, null, ["what's using my disk?"]), folder, "ask");
        SaveWpf(new AskWindow(AskKind.Selection, "error[E0308]: mismatched types", null, QuickActions.All(install.Paths.PromptsFile)), folder, "ask-selection");
        SaveWpf(new AskWindow(AskKind.Screen, null, null, []), folder, "ask-screen");
        var disk = "What is using my disk?";
        SaveWpf(new AnswerWindow(disk, new AskOutcome(true,
            "Your disk is mostly **photos** (41 GB) and the package cache (12 GB).\n\n- Pictures: 41 GB\n- Downloads: 12 GB\n\nEmpty the Downloads folder with `cleanmgr` to free the most space.",
            null, false, ServedBy: new ServedByInfo("kilo", "free", 3600))), folder, "answer");
        SaveWpf(new AnswerWindow(disk, new AskOutcome(false,
            "That didn't finish: every AI provider refused (rate limit).", null, false)), folder, "answer-failed");
        SaveWpf(new AnswerWindow(disk), folder, "answer-waiting");
        var streaming = new AnswerWindow(disk);
        streaming.ShowPartial("Your disk is mostly photos (41 GB) and the package");
        SaveWpf(streaming, folder, "answer-streaming");
        SaveWpf(new AnswerWindow("Close the Downloads window", new AskOutcome(true,
            "close_window: File Explorer · Downloads", null, false, Confirmation: new NeedsConfirmation("desktop", "close_window"))), folder, "answer-confirm");
        SaveMenu(folder, "tray-menu");
        SaveWpf(new EgressWindow(install.Paths), folder, "what-left-this-pc");
        SaveWpf(new ProviderWindow(install.Paths.EnvFile), folder, "ai-provider");
        SaveWpf(new AccountsWindow(install.Paths.EnvFile), folder, "accounts");
        return 0;
    }

    private static void Save(Form form, string folder, string name)
    {
        try
        {
            form.StartPosition = FormStartPosition.CenterScreen;
            Console.WriteLine($"[screenshots] {name}: show");
            form.Show();
            Console.WriteLine($"[screenshots] {name}: shown");
            form.Activate();
            Pump(1000);
            Console.WriteLine($"[screenshots] {name}: settled");
            var b = form.Bounds;
            using var bmp = new Bitmap(b.Width, b.Height);
            using (var g = Graphics.FromImage(bmp))
                g.CopyFromScreen(b.Location, Point.Empty, b.Size);
            bmp.Save(Path.Combine(folder, name + ".png"), ImageFormat.Png);
            Console.WriteLine($"[screenshots] {name}.png {b.Width}x{b.Height}");
        }
        catch (Exception e)
        {
            Console.WriteLine($"[screenshots] {name} failed: {e.Message}");
        }
        finally
        {
            form.Close();
            form.Dispose();
        }
    }

    private static bool _firstWpf = true;

    /// <summary>The tray menu, opened at a fixed spot with sample entries (nothing runs).</summary>
    private static void SaveMenu(string folder, string name)
    {
        void Nothing() { }
        var menu = TrayMenu.Open(
        [
            new("Ask FRIDAY", Wpf.Ui.Controls.SymbolRegular.Sparkle24, Nothing, "Win+Y", Bold: true),
            new("Ask about selection", Wpf.Ui.Controls.SymbolRegular.TextQuote24, Nothing, "Win+Alt+Y"),
            new("Ask about the screen", Wpf.Ui.Controls.SymbolRegular.Desktop24, Nothing, "Win+Ctrl+Y"),
            null,
            new("Last answer", Wpf.Ui.Controls.SymbolRegular.ChatBubblesQuestion24, Nothing),
            new("Name the app when asking about a selection", Wpf.Ui.Controls.SymbolRegular.AppGeneric24, Nothing, Checked: true),
            null,
            new("Settings…", Wpf.Ui.Controls.SymbolRegular.Settings24, Nothing),
            new("Quit FRIDAY", Wpf.Ui.Controls.SymbolRegular.Dismiss24, Nothing),
        ], new System.Windows.Point(200, 150));
        try
        {
            Pump(1200);
            var toDevice = System.Windows.PresentationSource.FromVisual(menu)?.CompositionTarget?.TransformToDevice
                ?? System.Windows.Media.Matrix.Identity;
            var at = menu.PointToScreen(new System.Windows.Point(0, 0));
            var b = new Rectangle((int)at.X - 16, (int)at.Y - 16,
                (int)Math.Round(menu.ActualWidth * toDevice.M11) + 32, (int)Math.Round(menu.ActualHeight * toDevice.M22) + 32);
            using var bmp = new Bitmap(b.Width, b.Height);
            using (var g = Graphics.FromImage(bmp))
                g.CopyFromScreen(b.Location, Point.Empty, b.Size);
            bmp.Save(Path.Combine(folder, name + ".png"), ImageFormat.Png);
            Console.WriteLine($"[screenshots] {name}.png {b.Width}x{b.Height}");
        }
        catch (Exception e)
        {
            Console.WriteLine($"[screenshots] {name} failed: {e.Message}");
        }
        finally
        {
            menu.IsOpen = false;
            Pump(200);
        }
    }

    /// <summary>A WPF window: shown where it places itself, captured once its entrance has settled.</summary>
    private static void SaveWpf(System.Windows.Window window, string folder, string name)
    {
        try
        {
            Console.WriteLine($"[screenshots] {name}: show");
            FluentApp.Show(window);
            window.Activate();
            // The entrance settles; the first WPF window also loads WPF itself.
            Pump(_firstWpf ? 4000 : 1400);
            _firstWpf = false;
            var toDevice = System.Windows.PresentationSource.FromVisual(window)?.CompositionTarget?.TransformToDevice
                ?? System.Windows.Media.Matrix.Identity;
            var b = new Rectangle(
                (int)Math.Round(window.Left * toDevice.M11), (int)Math.Round(window.Top * toDevice.M22),
                (int)Math.Round(window.ActualWidth * toDevice.M11), (int)Math.Round(window.ActualHeight * toDevice.M22));
            using var bmp = new Bitmap(b.Width, b.Height);
            using (var g = Graphics.FromImage(bmp))
                g.CopyFromScreen(b.Location, Point.Empty, b.Size);
            bmp.Save(Path.Combine(folder, name + ".png"), ImageFormat.Png);
            Console.WriteLine($"[screenshots] {name}.png {b.Width}x{b.Height}");
        }
        catch (Exception e)
        {
            Console.WriteLine($"[screenshots] {name} failed: {e.Message}");
        }
        finally
        {
            window.Close();
            Pump(200);
        }
    }

    /// <summary>Runs WPF's own loop (it also pumps the WinForms windows) for <paramref name="ms"/>.</summary>
    private static void Pump(int ms)
    {
        var frame = new System.Windows.Threading.DispatcherFrame();
        var timer = new System.Windows.Threading.DispatcherTimer { Interval = TimeSpan.FromMilliseconds(ms) };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            frame.Continue = false;
        };
        timer.Start();
        System.Windows.Threading.Dispatcher.PushFrame(frame);
    }
}
