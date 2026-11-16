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
        foreach (var name in new[] { "General", "Privacy", "AI", "Desktop control", "Shortcuts", "About" })
        {
            var b = new Button
            {
                Text = name,
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
        Show(section is not null && _nav.ContainsKey(section) ? section : "General");
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
        column.Controls.Add(new Label { Text = name, Font = new Font("Segoe UI Semibold", 15f), ForeColor = Theme.Text, AutoSize = true, Margin = new Padding(0, 0, 0, 14) });
        foreach (var c in name switch
        {
            "General" => General(),
            "Privacy" => Privacy(),
            "AI" => Ai(),
            "Desktop control" => Desktop(),
            "Shortcuts" => Shortcuts(),
            _ => About(),
        })
            column.Controls.Add(c);
        _page.Controls.Add(column);
        _page.ResumeLayout();
    }

    // ---- pages ----

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
                "Windows will ask for permission once now. Turn it off here any time.\n\nContinue?",
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
