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
            () => { }, () => { }, _ => { }, () => { }, keys);

        foreach (var section in new[] { "Home", "General", "Privacy", "AI", "Voice", "Desktop control", "Shortcuts", "Health & updates", "About" })
            Save(new SettingsForm(install, nothing, section), folder, "settings-" + section.Replace(" & ", "-").Replace(' ', '-').ToLowerInvariant());
        Save(new WelcomeForm(keys, () => { }, () => { }, () => { }), folder, "welcome");
        Save(new AskForm(AskKind.Plain, null, ["what's using my disk?"]), folder, "ask");
        Save(new AskForm(AskKind.Selection, "error[E0308]: mismatched types", QuickActions.All(install.Paths.PromptsFile)), folder, "ask-selection");
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
        Save(new EgressForm(install.Paths), folder, "what-left-this-pc");
        Save(new ProviderForm(install.Paths.EnvFile), folder, "ai-provider");
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
