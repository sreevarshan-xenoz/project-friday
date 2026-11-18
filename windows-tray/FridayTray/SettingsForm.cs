using FridayTray.Core;

namespace FridayTray;

/// <summary>What the Settings window asks the tray to do (it owns the daemon and the other windows).</summary>
internal sealed record SettingsActions(
    Action RestartDaemon,
    Action ChooseProvider,
    Action ConnectAccounts,
    Action ShowEgress,
    Action ForgetSavedData,
    Action OpenLogs,
    Action OpenDataFolder,
    Action RestartIntoAdministrator,
    Action<string> AskText,
    Action ShowWelcome,
    IReadOnlyDictionary<AskKind, string> Hotkeys);

/// <summary>
/// FRIDAY's settings as a normal window: sections on the left, switches and
/// buttons on the right. A switch takes effect as soon as it is flipped
/// (friday.env is rewritten and the daemon restarted when that is what it
/// changes); nothing here is a second copy of a setting, so the tray menu and
/// friday.env always agree with it.
/// </summary>
internal sealed class SettingsForm : Form
{
    private readonly Install _install;
    private readonly SettingsActions _do;
    private readonly Panel _page = new() { Dock = DockStyle.Fill, BackColor = Theme.Bg, Padding = new Padding(28, 22, 28, 18), AutoScroll = true };
    private readonly Dictionary<string, Button> _nav = new();
    private readonly Label _status = new() { AutoSize = true, ForeColor = Theme.Signal, Font = Theme.Small, Dock = DockStyle.Bottom };

    public SettingsForm(Install install, SettingsActions actions, string? section = null)
    {
        Theme.DarkTitleBar(this);
        _install = install;
        _do = actions;
        Text = "FRIDAY · Settings";
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(760, 520);
        MinimumSize = new Size(700, 480);
        Icon = Theme.Orb("ready");

        var side = new Panel { Dock = DockStyle.Left, Width = 190, BackColor = Theme.Surface, Padding = new Padding(10, 18, 10, 10) };
        var brand = new Label { Text = "FRIDAY", Font = Theme.Title, ForeColor = Theme.Accent, Dock = DockStyle.Top, Height = 34 };
        var items = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false, BackColor = Theme.Surface };
        foreach (var name in new[] { "Home", "General", "Privacy", "AI", "Voice", "Desktop control", "Shortcuts", "Health & updates", "About" })
        {
            var b = new Button
            {
                Text = name,
                UseMnemonic = false, // "Health & updates", not "Health _updates"
                Width = 168,
                Height = 36,
                TextAlign = ContentAlignment.MiddleLeft,
                FlatStyle = FlatStyle.Flat,
                ForeColor = Theme.Text,
                BackColor = Theme.Surface,
                Font = Theme.Body,
                Padding = new Padding(10, 0, 0, 0),
                Margin = new Padding(0, 0, 0, 2),
                Cursor = Cursors.Hand,
            };
            b.FlatAppearance.BorderSize = 0;
            b.Click += (_, _) => Show(name);
            _nav[name] = b;
            items.Controls.Add(b);
        }
        side.Controls.Add(items);
        side.Controls.Add(brand);

        Controls.Add(_page);
        Controls.Add(side);
        Controls.Add(_status);
        Show(section is not null && _nav.ContainsKey(section) ? section : "Home");
    }

    private void Show(string name)
    {
        foreach (var (n, b) in _nav)
        {
            b.BackColor = n == name ? Theme.Raised : Theme.Surface;
            b.ForeColor = n == name ? Theme.Accent : Theme.Text;
        }
        _status.Text = "";
        _page.SuspendLayout();
        _page.Controls.Clear();
        var column = new FlowLayoutPanel { Dock = DockStyle.Top, AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false, BackColor = Theme.Bg };
        column.Controls.Add(new Label { Text = name, UseMnemonic = false, Font = new Font("Segoe UI Semibold", 15f), ForeColor = Theme.Text, AutoSize = true, Margin = new Padding(0, 0, 0, 14) });
        foreach (var c in name switch
        {
            "Home" => Home(),
            "General" => General(),
            "Privacy" => Privacy(),
            "AI" => Ai(),
            "Voice" => Voice(),
            "Desktop control" => Desktop(),
            "Shortcuts" => Shortcuts(),
            "Health & updates" => HealthAndUpdates(),
            _ => About(),
        })
            column.Controls.Add(c);
        _page.Controls.Add(column);
        _page.ResumeLayout();
    }

    // ---- pages ----

    private IEnumerable<Control> Home()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        yield return Pair("FRIDAY", DaemonState.Read(_install.Paths.StateFile).Detail);
        yield return Pair("AI", AppSettings.UsingFreeModels(env)
            ? "Free models (they may keep what you send)"
            : "Your own account or key");
        yield return Pair("Voice", !Speaking.Installed(_install.Paths) ? "Not set up"
            : Speaking.Enabled(_install.Paths) ? "On" : "Off");
        yield return Note("Press " + (_do.Hotkeys.TryGetValue(AskKind.Plain, out var k) ? k : "the tray icon") + " any time to ask FRIDAY anything.");
        yield return Row(Theme.Button("Ask FRIDAY something", primary: true), () => _do.AskText(""));
        yield return Row(Theme.Button("How is my computer doing?"), () => _do.AskText("How is my computer doing?"));
        yield return Row(Theme.Button("Show the welcome again"), _do.ShowWelcome);
    }

    private IEnumerable<Control> HealthAndUpdates()
    {
        var v = typeof(SettingsForm).Assembly.GetName().Version;
        yield return Pair("This version", $"{v?.Major}.{v?.Minor}.{v?.Build}");
        yield return Note("FRIDAY doesn't update itself. To get the newest version, download it and run it over this one; your settings and data are kept.");
        yield return Row(Theme.Button("Open the download page", primary: true),
            () => System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(
                "https://github.com/sreevarshan-xenoz/project-friday/releases/latest") { UseShellExecute = true }));
        yield return Row(Theme.Button("Check my computer's health"), () => _do.AskText("How is my computer doing?"));
        yield return Row(Theme.Button("Restart FRIDAY"), () => { _do.RestartDaemon(); Said("FRIDAY restarted."); });
        yield return Note("Something wrong? Save a short report to your desktop to send to whoever is helping. It has versions and which switches are on; no questions, answers, logs or keys.");
        yield return Row(Theme.Button("Save a support report"), SaveReport);
    }

    private void SaveReport()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        var v = typeof(SettingsForm).Assembly.GetName().Version;
        var voice = !Speaking.Installed(_install.Paths) ? "not set up" : Speaking.Enabled(_install.Paths) ? "on" : "off";
        var text = SupportReport.Build($"{v?.Major}.{v?.Minor}.{v?.Build}", Environment.OSVersion.VersionString,
            DaemonState.Read(_install.Paths.StateFile).Detail, AppSettings.UsingFreeModels(env),
            AppSettings.DesktopControlOn(env), AppSettings.ScreenTextOn(env), voice);
        var file = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "FRIDAY support report.txt");
        try
        {
            File.WriteAllText(file, text);
            Said("Saved to your desktop: FRIDAY support report.txt");
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            MessageBox.Show(this, "Couldn't save the report: " + e.Message, "FRIDAY", MessageBoxButtons.OK, MessageBoxIcon.Warning);
        }
    }

    private IEnumerable<Control> General()
    {
        var admin = Elevation.Enabled();
        yield return Switch(
            "Start FRIDAY when I sign in",
            admin
                ? "Administrator start-up (below) is on, so Windows starts FRIDAY at sign-in through its task scheduler."
                : "FRIDAY waits in the tray so Win+Y works right away.",
            _install.StartsAtLogin || admin,
            on => _install.StartsAtLogin = on,
            enabled: !admin);
        yield return Switch(
            "Start with administrator rights",
            "Lets FRIDAY control apps that run as administrator (an app like that ignores keys and clicks from a normal program). " +
            "Everything FRIDAY does then runs with those rights, so only turn it on if you need it. " +
            "Windows asks for permission once, now; after that FRIDAY starts at every sign-in without asking. " +
            (Elevation.IsElevated() ? "FRIDAY is running as administrator now." : "Takes effect the next time FRIDAY starts."),
            admin,
            on => SetAdministrator(on));
        yield return Row(Theme.Button("Restart FRIDAY"), () => { _do.RestartDaemon(); Said("FRIDAY restarted."); });
        yield return Row(Theme.Button("Open logs"), _do.OpenLogs);
    }

    private IEnumerable<Control> Privacy()
    {
        yield return Switch(
            "Name the app when asking about a selection",
            "A question about selected text says which app (and window title) it came from, so answers fit. Off: only the text is sent.",
            SourceApp.Enabled(_install.Paths),
            on => SourceApp.SetEnabled(_install.Paths, on));
        yield return Switch(
            "Read a window's text for screen questions",
            "When you ask about your screen, FRIDAY may send the window's text instead of a screenshot, if the app offers enough of it. " +
            "That text can include words you can't see and goes to the AI provider you use. Password fields are skipped; key-like secrets are masked. Off by default.",
            AppSettings.ScreenTextOn(EnvFile.Load(_install.Paths.EnvFile)),
            on =>
            {
                if (on && MessageBox.Show(this,
                        "FRIDAY would send a window's text (it can include words that are not on screen) to your AI provider instead of a screenshot.\n\nTurn it on?",
                        "Read a window's text", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) != DialogResult.Yes)
                {
                    Show("Privacy");
                    return;
                }
                ChangeEnv(env => AppSettings.WithScreenText(env, on));
            });
        yield return Row(Theme.Button("What left this PC…"), _do.ShowEgress);
        yield return Row(Theme.Button("Forget saved data…"), _do.ForgetSavedData);
    }

    private IEnumerable<Control> Ai()
    {
        yield return Note("FRIDAY thinks with a language model. Free models work with no account; for anything private use your own key or account. Your key is stored only on this PC, in your profile.");
        yield return Row(Theme.Button("Choose a provider and key…", primary: true), _do.ChooseProvider);
        yield return Row(Theme.Button("Connect an account…"), _do.ConnectAccounts);
    }

    private Button Download(string text, bool primary, string[] setup)
    {
        var button = Theme.Button(text, primary: primary);
        button.Click += async (_, _) =>
        {
            button.Enabled = false;
            Said("Downloading the voice…");
            var (ok, output) = await Ctl.RunAsync(_install, setup, TimeSpan.FromMinutes(15));
            if (ok)
                Show("Voice");
            else
            {
                button.Enabled = true;
                Said("");
                MessageBox.Show(this, "The voice wasn't installed:\n\n" + output.Trim(), "FRIDAY", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        };
        return button;
    }

    private IEnumerable<Control> Voice()
    {
        var paths = _install.Paths;
        if (!Speaking.Installed(paths))
        {
            yield return Note("FRIDAY can read its answers out loud in a voice that is made on this PC: nothing is sent anywhere to speak. " +
                "It downloads once (about 50 MB, checked against a fixed checksum) and runs on the CPU. The voice (tiny voca, female or male) is clear and natural, and starts speaking at once.");
            yield return Download("Download FRIDAY's voice", primary: true, setup: ["say", "--setup"]);
            yield return Note("Prefer a more expressive voice? Kokoro (Bella, and seven more to choose from) is about 150 MB, takes a few seconds before the first words and about 1 GB of memory while it speaks.");
            yield return Download("Download the Kokoro voice", primary: false, setup: ["say", "--setup", "kokoro"]);
            yield break;
        }
        yield return Switch(
            "Read answers out loud",
            "FRIDAY reads each answer as it appears, a sentence at a time. Closing the answer stops it. Code is skipped.",
            Speaking.Enabled(paths),
            on => Speaking.SetEnabled(paths, on));
        var picker = new ComboBox
        {
            DropDownStyle = ComboBoxStyle.DropDownList,
            Width = 220,
            BackColor = Theme.Surface,
            ForeColor = Theme.Text,
            FlatStyle = FlatStyle.Flat,
            Margin = new Padding(0, 0, 0, 10),
        };
        for (var i = 0; i < Speaking.Voices; i++)
            picker.Items.Add($"Voice {i + 1}");
        picker.SelectedIndex = Speaking.Speaker(paths);
        var loading = false;
        picker.SelectedIndexChanged += (_, _) =>
        {
            if (loading || picker.SelectedIndex < 0)
                return;
            Speaking.SetSpeaker(paths, picker.SelectedIndex);
            Ctl.Say(_install, "Hi, I'm FRIDAY. This is how I sound.");
        };
        // The installed voice's own names (Bella, Heart, ...) replace "Voice 1, 2, ...".
        _ = Task.Run(async () =>
        {
            var (ok, output) = await Ctl.RunAsync(_install, ["say", "--voices"], TimeSpan.FromSeconds(15));
            var names = ok ? Speaking.ParseVoices(output) : [];
            if (names.Count == 0 || picker.IsDisposed)
                return;
            picker.BeginInvoke(() =>
            {
                loading = true;
                var keep = Math.Min(Speaking.Speaker(paths), names.Count - 1);
                picker.Items.Clear();
                picker.Items.AddRange(names.Cast<object>().ToArray());
                picker.SelectedIndex = keep;
                loading = false;
            });
        });
        yield return Pair("Voice", "");
        yield return picker;
        yield return Row(Theme.Button("Hear it"), () => Ctl.Say(_install, "Hi, I'm FRIDAY. This is how I sound."));
        yield return Row(Theme.Button("Stop"), () => Ctl.StopSaying(_install));
    }

    private IEnumerable<Control> Desktop()
    {
        yield return Switch(
            "Let FRIDAY control my desktop when I ask",
            "Say \"focus Notepad\", \"open Brave\", \"set the volume to 30\", \"pause the music\", \"switch to desktop 2\". " +
            "FRIDAY only does this when you ask; closing a window always asks first and shows the exact action. Off: FRIDAY answers questions but does not act.",
            AppSettings.DesktopControlOn(EnvFile.Load(_install.Paths.EnvFile)),
            on => ChangeEnv(env => AppSettings.WithDesktopControl(env, on)));
    }

    private IEnumerable<Control> Shortcuts()
    {
        yield return Note("The keys FRIDAY answers to. If another program already owns a key, FRIDAY picks the next free one and shows it here.");
        foreach (var (kind, label) in new[]
        {
            (AskKind.Plain, "Ask FRIDAY"),
            (AskKind.Selection, "Ask about the selected text"),
            (AskKind.Screen, "Ask about the screen"),
        })
            yield return Pair(label, _do.Hotkeys.TryGetValue(kind, out var key) ? key : "not available (taken by another program)");
        yield return Note("Changing the keys is not available yet.");
    }

    private IEnumerable<Control> About()
    {
        var v = typeof(SettingsForm).Assembly.GetName().Version;
        yield return Pair("FRIDAY", $"{v?.Major}.{v?.Minor}.{v?.Build}");
        yield return Pair("Installed in", _install.Dir.TrimEnd('\\'));
        yield return Pair("Your data", _install.Paths.EnvFile.Replace("\\friday.env", ""));
        yield return Row(Theme.Button("Open the data folder"), _do.OpenDataFolder);
    }

    // ---- building blocks ----

    private Control Switch(string title, string detail, bool on, Action<bool> set, bool enabled = true)
    {
        var box = new CheckBox
        {
            Text = title,
            Checked = on,
            Enabled = enabled,
            AutoSize = true,
            FlatStyle = FlatStyle.Flat,
            ForeColor = enabled ? Theme.Text : Theme.Subtext,
            Font = Theme.Title,
            Margin = new Padding(0, 0, 0, 2),
        };
        var note = new Label
        {
            Text = detail,
            AutoSize = true,
            MaximumSize = new Size(480, 0),
            ForeColor = Theme.Subtext,
            Font = Theme.Small,
            Margin = new Padding(22, 0, 0, 16),
        };
        box.CheckedChanged += (_, _) => set(box.Checked);
        var cell = new FlowLayoutPanel { AutoSize = true, FlowDirection = FlowDirection.TopDown, WrapContents = false, BackColor = Theme.Bg };
        cell.Controls.Add(box);
        cell.Controls.Add(note);
        return cell;
    }

    private static Control Note(string text) => new Label
    {
        Text = text,
        AutoSize = true,
        MaximumSize = new Size(480, 0),
        ForeColor = Theme.Subtext,
        Font = Theme.Small,
        Margin = new Padding(0, 0, 0, 14),
    };

    private static Control Row(Button button, Action click)
    {
        button.Click += (_, _) => click();
        button.Margin = new Padding(0, 0, 0, 10);
        return button;
    }

    private static Control Pair(string name, string value)
    {
        var row = new FlowLayoutPanel { AutoSize = true, FlowDirection = FlowDirection.LeftToRight, WrapContents = false, BackColor = Theme.Bg, Margin = new Padding(0, 0, 0, 8) };
        row.Controls.Add(new Label { Text = name, Width = 220, ForeColor = Theme.Subtext, AutoSize = false, Height = 24 });
        row.Controls.Add(new Label { Text = value, AutoSize = true, MaximumSize = new Size(300, 0), ForeColor = Theme.Text });
        return row;
    }

    private void Said(string text) => _status.Text = text;

    /// <summary>Rewrites friday.env with <paramref name="change"/> and restarts the daemon so it takes effect.</summary>
    private void ChangeEnv(Func<string, string> change)
    {
        var path = _install.Paths.EnvFile;
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, change(File.Exists(path) ? File.ReadAllText(path) : ""));
        File.Move(tmp, path, overwrite: true);
        _do.RestartDaemon();
        Said("Saved. FRIDAY restarted.");
    }

    private void SetAdministrator(bool on)
    {
        if (on && MessageBox.Show(this,
                "FRIDAY will start with administrator rights at every sign-in, so it can control apps that run as administrator.\n\n" +
                "Everything FRIDAY does will then run with administrator rights. FRIDAY acts on what an AI model decides, so a misread request, " +
                "or text on your screen that tricks it, would act with those rights too. Closing a window and anything that deletes or sends still asks you first.\n\n" +
                "Only turn this on if you need to control administrator apps. Windows will ask for permission once now; you can turn it off here any time.\n\nContinue?",
                "Start with administrator rights", MessageBoxButtons.OKCancel, MessageBoxIcon.Information) != DialogResult.OK)
        {
            Show("General");
            return;
        }
        if (!Elevation.Apply(_install, on))
        {
            MessageBox.Show(this, "Windows didn't allow the change (the prompt was declined, or this account can't approve it). Nothing was changed.",
                "FRIDAY", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            Show("General");
            return;
        }
        // The task now starts FRIDAY at sign-in (or the Run entry does again).
        _install.StartsAtLogin = !on;
        Show("General");
        if (MessageBox.Show(this, on ? "FRIDAY will start as administrator from now on. Restart it now?" : "FRIDAY will start normally from now on. Restart it now?",
                "FRIDAY", MessageBoxButtons.YesNo, MessageBoxIcon.Question) == DialogResult.Yes)
            _do.RestartIntoAdministrator();
    }
}
