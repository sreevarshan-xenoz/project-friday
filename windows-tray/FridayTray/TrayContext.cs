using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// The running tray app: the icon and its menu, the hotkeys, the ask flow,
/// and the two supervised programs (friday-daemon, friday-windows-host).
/// </summary>
internal sealed class TrayContext : ApplicationContext
{
    private readonly Install _install;
    private readonly AnswerLog _log;
    private readonly NotifyIcon _icon;
    private readonly Hotkeys _hotkeys = new();
    private readonly Supervised _daemon;
    private readonly Supervised _host;
    private readonly System.Windows.Forms.Timer _poll = new() { Interval = 2000 };
    private readonly ToolStripMenuItem _startAtLogin;
    private string _mode = "";
    private bool _asking;

    public TrayContext(Install install)
    {
        _install = install;
        _log = new AnswerLog(install.Paths);
        _daemon = new Supervised("friday-daemon", install.Daemon, install.ChildEnvironment, install.Paths.LogDir);
        _host = new Supervised("friday-windows-host", install.Host, install.ChildEnvironment, install.Paths.LogDir);
        _daemon.GaveUp += name => Notify("FRIDAY keeps crashing", $"Stopped restarting {name}. Logs: {install.Paths.LogDir}", ToolTipIcon.Error);
        _host.GaveUp += name => Notify("FRIDAY keeps crashing", $"Stopped restarting {name}. Logs: {install.Paths.LogDir}", ToolTipIcon.Error);

        _hotkeys.Pressed += kind => _ = AskAsync(kind);
        _hotkeys.RegisterAll();
        string Key(AskKind k) => _hotkeys.Labels.TryGetValue(k, out var l) ? $"\t{l}" : "";

        _startAtLogin = new ToolStripMenuItem("Start at login", null, (_, _) => ToggleStartAtLogin()) { Checked = install.StartsAtLogin };
        var menu = new ContextMenuStrip();
        menu.Items.Add(new ToolStripMenuItem("Ask FRIDAY" + Key(AskKind.Plain), null, (_, _) => _ = AskAsync(AskKind.Plain)) { Font = new Font(menu.Font, FontStyle.Bold) });
        menu.Items.Add("Ask about selection" + Key(AskKind.Selection), null, (_, _) => _ = AskAsync(AskKind.Selection));
        menu.Items.Add("Ask about the screen" + Key(AskKind.Screen), null, (_, _) => _ = AskAsync(AskKind.Screen));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Last answer", null, (_, _) => Open(install.Paths.LastAnswerFile));
        menu.Items.Add("All answers", null, (_, _) => Open(install.Paths.AnswersFile));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("AI provider…", null, (_, _) => ChooseProvider());
        menu.Items.Add(_startAtLogin);
        menu.Items.Add("Restart FRIDAY", null, (_, _) => RestartChildren());
        menu.Items.Add("Open logs", null, (_, _) => Open(install.Paths.LogDir));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Quit FRIDAY", null, (_, _) => ExitThread());

        _icon = new NotifyIcon { ContextMenuStrip = menu, Visible = true, Text = "FRIDAY" };
        _icon.MouseClick += (_, e) => { if (e.Button == MouseButtons.Left) _ = AskAsync(AskKind.Plain); };
        UpdateStatus();
        _poll.Tick += (_, _) => UpdateStatus();
        _poll.Start();

        _host.Start();
        _daemon.Start();

        if (!_hotkeys.Labels.ContainsKey(AskKind.Plain))
            Notify("FRIDAY", "Another program owns FRIDAY's hotkeys. Click the tray icon to ask.", ToolTipIcon.Warning);
        else
            Notify("FRIDAY is running", $"Press {_hotkeys.Labels[AskKind.Plain]} to ask anything.", ToolTipIcon.Info);
    }

    private async Task AskAsync(AskKind kind)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            string? context = null;
            string? preview = null;
            if (kind == AskKind.Selection)
            {
                var sel = await Selection.CaptureAsync();
                if (sel.Problem != null)
                {
                    Notify("FRIDAY", sel.Problem, ToolTipIcon.Info);
                    return;
                }
                context = sel.Text!.Length > Asking.MaxContextChars ? sel.Text[..Asking.MaxContextChars] : sel.Text;
                preview = $"{sel.Source}: {context.ReplaceLineEndings(" ")}";
                preview = preview.Length > 60 ? preview[..60] : preview;
            }

            string question;
            using (var form = new AskForm(kind, preview, kind == AskKind.Plain ? _log.History() : []))
            {
                if (form.ShowDialog() != DialogResult.OK)
                    return;
                question = form.Question;
            }
            _log.Remember(question);

            var request = Asking.BuildRequest(question, context);
            await RunRequestAsync(kind, question, context, request, confirm: null, answer: null);
        }
        finally
        {
            _asking = false;
        }
    }

    private async Task RunRequestAsync(AskKind kind, string question, string? context, string request, string? confirm, AnswerForm? answer)
    {
        // The answer window opens now and fills in while the answer
        // streams (state.json's current.partial). Not for a screen ask:
        // FRIDAY captures the focused window, which must still be the one
        // asked about, so that window opens with the first words.
        using var poll = new System.Windows.Forms.Timer { Interval = 250 };
        string? shown = null;
        if (kind == AskKind.Screen)
        {
            Notify("FRIDAY", "Looking at your screen: " + question, ToolTipIcon.None);
        }
        else
        {
            if (answer is null || answer.IsDisposed)
                answer = NewAnswerForm(kind, question, "Thinking…");
            else
                answer.ShowWaiting("Thinking…");

            poll.Tick += (_, _) =>
            {
                var partial = ReadPartial(request);
                if (partial != null && partial != shown && !answer.IsDisposed)
                {
                    answer.ShowPartial(partial);
                    shown = partial;
                }
            };
            poll.Start();
        }
        // A screen answer streams through friday-ctl's own output; its
        // window opens with the first words (the capture is done by then).
        var ui = SynchronizationContext.Current;
        Action<string>? onLine = kind != AskKind.Screen || ui is null ? null : line =>
        {
            if (Asking.PartialLine(line) is not { } partial)
                return;
            ui.Post(_ =>
            {
                if (answer is null || answer.IsDisposed)
                    answer = NewAnswerForm(kind, question, "");
                answer.ShowPartial(partial);
            }, null);
        };
        var (ok, output, exitCode) = await Ctl.RunAsync(
            _install,
            Asking.CtlArguments(kind, question, request, confirm),
            TimeSpan.FromSeconds(240),
            onLine);
        poll.Stop();
        var outcome = Asking.Interpret(kind, question, exitCode, output);
        _log.Save(question, context, outcome, DateTime.Now);

        // A screen ask, or closed while it was on its way: show it now.
        if (answer is null || answer.IsDisposed)
            answer = NewAnswerForm(kind, question, "");

        if (outcome.Confirmation is { } nextConf)
        {
            Action<string>? onConfirm = null;
            onConfirm = action =>
            {
                answer.ConfirmRun -= onConfirm;
                _ = ExecuteConfirmedAsync(kind, question, context, request, action, answer);
            };
            answer.ConfirmRun += onConfirm;
        }

        answer.ShowOutcome(outcome);
        answer.Activate();
    }

    private async Task ExecuteConfirmedAsync(AskKind kind, string question, string? context, string request, string confirmAction, AnswerForm answer)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            await RunRequestAsync(kind, question, context, request, confirmAction, answer);
        }
        finally
        {
            _asking = false;
        }
    }

    private AnswerForm NewAnswerForm(AskKind kind, string question, string waiting)
    {
        var form = new AnswerForm(question, waiting);
        form.AskAgain += () => _ = AskAsync(kind);
        form.StartFriday += RestartChildren;
        form.Show();
        form.Activate();
        return form;
    }

    /// <summary>The streaming answer to <paramref name="request"/> so far, if any.</summary>
    private string? ReadPartial(string request)
    {
        try
        {
            using var fs = new FileStream(_install.Paths.StateFile, FileMode.Open, FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(fs);
            return Asking.PartialAnswer(reader.ReadToEnd(), request);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    private void UpdateStatus()
    {
        var state = DaemonState.Read(_install.Paths.StateFile);
        var text = state.Detail.Length > 60 ? state.Detail[..60] : state.Detail;
        _icon.Text = text;
        if (state.Mode == _mode)
            return;
        _mode = state.Mode;
        var old = _icon.Icon;
        _icon.Icon = Theme.Orb(state.Mode);
        old?.Dispose();
    }

    private void ChooseProvider()
    {
        using var form = new ProviderForm(_install.Paths.EnvFile);
        if (form.ShowDialog() == DialogResult.OK)
        {
            _daemon.Restart();
            Notify("FRIDAY", "Saved. FRIDAY restarted with the new provider.", ToolTipIcon.Info);
        }
    }

    private void ToggleStartAtLogin()
    {
        _install.StartsAtLogin = !_install.StartsAtLogin;
        _startAtLogin.Checked = _install.StartsAtLogin;
    }

    private void RestartChildren()
    {
        _host.Restart();
        _daemon.Restart();
        Notify("FRIDAY", "Restarted.", ToolTipIcon.Info);
    }

    private static void Open(string path)
    {
        if (!File.Exists(path) && !Directory.Exists(path))
            Directory.CreateDirectory(Path.GetDirectoryName(path) ?? path);
        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(path) { UseShellExecute = true });
        }
        catch (System.ComponentModel.Win32Exception) { }
    }

    private void Notify(string title, string text, ToolTipIcon icon) =>
        _icon.ShowBalloonTip(5000, title, text.Length > 250 ? text[..250] : text, icon);

    protected override void ExitThreadCore()
    {
        _poll.Stop();
        _hotkeys.Dispose();
        _daemon.Dispose();
        _host.Dispose();
        _icon.Visible = false;
        _icon.Dispose();
        base.ExitThreadCore();
    }
}
