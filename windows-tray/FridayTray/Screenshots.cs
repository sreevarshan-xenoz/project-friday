using System.Drawing.Imaging;
using FridayTray.Core;

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
        ApplicationConfiguration.Initialize();
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
        Save(new AskForm(AskKind.Selection, "error[E0308]: mismatched types", []), folder, "ask-selection");
        Save(new AnswerForm("What is using my disk?", new AskOutcome(true,
            "Your disk is mostly **photos** (41 GB) and the package cache (12 GB).\n\n- Pictures: 41 GB\n- Downloads: 12 GB\n\nEmpty the Downloads folder to free the most space.",
            null, false)), folder, "answer");
        Save(new AnswerForm("What is using my disk?", new AskOutcome(false,
            "That didn't finish: every AI provider refused (rate limit).", null, false)), folder, "answer-failed");
        Save(new AnswerForm("What is using my disk?"), folder, "answer-waiting");
        Save(new EgressForm(install.Paths), folder, "what-left-this-pc");
        Save(new ProviderForm(install.Paths.EnvFile), folder, "ai-provider");
        return 0;
    }

    private static void Save(Form form, string folder, string name)
    {
        try
        {
            form.StartPosition = FormStartPosition.CenterScreen;
            form.Show();
            form.Activate();
            for (var i = 0; i < 12; i++)
            {
                Application.DoEvents();
                Thread.Sleep(80);
            }
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
}
