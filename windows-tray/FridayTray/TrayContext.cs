using Microsoft.Win32;
using FridayTray.Core;
using FridayTray.Fluent;

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
    private readonly ToolStripMenuItem _appContext;
    private readonly ToolStripMenuItem _screenText;
    private string _mode = "";
    private bool _asking;

    public TrayContext(Install install)
    {
        _install = install;

        // Last-resort unhandled exception handlers so an unexpected UI or background
        // exception is logged and keeps the tray alive rather than terminating.
        Application.ThreadException += (_, e) => LogUnhandledException("ThreadException", e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) =>
        {
            if (e.ExceptionObject is Exception ex)
                LogUnhandledException("UnhandledException", ex);
        };

        _log = new AnswerLog(install.Paths);
        ApplyInstallerChoices();
        _daemon = new Supervised("friday-daemon", install.Daemon, install.ChildEnvironment, install.Paths.LogDir);
        _host = new Supervised("friday-windows-host", install.Host, install.ChildEnvironment, install.Paths.LogDir);
        // GaveUp fires on the exited process's thread; the tray icon belongs to this one.
        var ui = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();
        void GaveUp(string name) => ui.Post(_ => Notify("FRIDAY keeps crashing", $"Stopped restarting {name}. Logs: {install.Paths.LogDir}", ToolTipIcon.Error), null);
        _daemon.GaveUp += GaveUp;
        _host.GaveUp += GaveUp;

        _hotkeys.Pressed += kind => _ = AskAsync(kind);
        _hotkeys.RegisterAll();
        // The hotkey is drawn right-aligned by the menu (a tab in the text would
        // not be expanded and ran into the name).
        ToolStripMenuItem Ask(string text, AskKind kind, bool pickArea = false) =>
            new(text, null, (_, _) => _ = AskAsync(kind, pickArea))
            {
                ShortcutKeyDisplayString = !pickArea && _hotkeys.Labels.TryGetValue(kind, out var l) ? l : null,
            };

        _startAtLogin = new ToolStripMenuItem("Start at login", null, (_, _) => ToggleStartAtLogin()) { Checked = install.StartsAtLogin };
        var menu = new ContextMenuStrip();
        var askItem = Ask("Ask FRIDAY", AskKind.Plain);
        askItem.Font = new Font(menu.Font, FontStyle.Bold);
        menu.Items.Add(askItem);
        menu.Items.Add(Ask("Ask about selection", AskKind.Selection));
        menu.Items.Add(Ask("Ask about the screen", AskKind.Screen));
        menu.Items.Add(Ask("Ask about an area…", AskKind.Screen, pickArea: true));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Last answer", null, (_, _) => Open(install.Paths.LastAnswerFile));
        menu.Items.Add("All answers", null, (_, _) => Open(install.Paths.AnswersFile));
        menu.Items.Add("What left this PC…", null, (_, _) => ShowEgress());
        menu.Items.Add("Forget saved data…", null, (_, _) => ForgetSavedData());
        _appContext = new ToolStripMenuItem("Name the app when asking about a selection", null, (_, _) => ToggleAppContext())
            { Checked = SourceApp.Enabled(install.Paths) };
        menu.Items.Add(_appContext);
        _screenText = new ToolStripMenuItem("Read a window's text for screen asks", null, (_, _) => ToggleScreenText())
            { Checked = ScreenTextOn() };
        menu.Items.Add(_screenText);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Welcome…", null, (_, _) => ShowWelcome());
        menu.Items.Add("Settings…", null, (_, _) => OpenSettings());
        menu.Items.Add("AI provider…", null, (_, _) => ChooseProvider());
        menu.Items.Add(_startAtLogin);
        menu.Items.Add("Restart FRIDAY", null, (_, _) => RestartChildren());
        menu.Items.Add("Open logs", null, (_, _) => Open(install.Paths.LogDir));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Quit FRIDAY", null, (_, _) => ExitThread());

        // The Settings window can change these; show what is true when the menu opens.
        menu.Opening += (_, _) =>
        {
            _appContext.Checked = SourceApp.Enabled(install.Paths);
            _screenText.Checked = ScreenTextOn();
            _startAtLogin.Checked = install.StartsAtLogin;
        };
        _icon = new NotifyIcon { ContextMenuStrip = menu, Visible = true, Text = "FRIDAY" };
        // Opening FRIDAY again (the Start menu entry) opens its window instead of doing nothing.
        var openWindow = new System.Threading.EventWaitHandle(false, System.Threading.EventResetMode.AutoReset, Program.OpenEventName);
        var uiContext = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();
        new System.Threading.Thread(() =>
        {
            while (openWindow.WaitOne())
                uiContext.Post(_ => OpenSettings(), null);
        }) { IsBackground = true, Name = "FRIDAY open-window listener" }.Start();
        _icon.MouseClick += (_, e) => { if (e.Button == MouseButtons.Left) _ = AskAsync(AskKind.Plain); };
        UpdateStatus();
        _poll.Tick += (_, _) => UpdateStatus();
        _poll.Start();

        _host.Start();
        _daemon.Start();

        if (_askForAdministrator && !Elevation.Enabled())
            AskForAdministrator();
        else if (!_hotkeys.Labels.ContainsKey(AskKind.Plain))
            Notify("FRIDAY", "Another program owns FRIDAY's hotkeys. Click the tray icon to ask.", ToolTipIcon.Warning);
        else if (Welcome.Pending(install.Paths))
            ShowWelcome();
        else
            Notify("FRIDAY is running", $"Press {_hotkeys.Labels[AskKind.Plain]} to ask anything.", ToolTipIcon.Info);
    }

    private WelcomeForm? _welcome;

    private void ShowWelcome()
    {
        if (_welcome is { IsDisposed: false })
        {
            _welcome.Activate();
            return;
        }
        Welcome.MarkShown(_install.Paths);
        _welcome = new WelcomeForm(_hotkeys.Labels, ChooseProvider, () => OpenSettings(), () => _ = AskAsync(AskKind.Plain));
        _welcome.FormClosed += (_, _) => _welcome = null;
        _welcome.Show();
    }

    private async Task AskAsync(AskKind kind, bool pickArea = false)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            // Drag out the area first: only that part of the screen is captured.
            PickedRegion? area = null;
            if (pickArea)
            {
                using var picker = new RegionForm();
                if (picker.ShowDialog() != DialogResult.OK)
                    return;
                area = picker.Picked;
            }
            var targetWindow = Native.GetForegroundWindow();
            string? context = null;
            string? preview = null;
            string? appLine = null;
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
                // Read now, while the window the text came from is still in front.
                if (SourceApp.Enabled(_install.Paths))
                    appLine = SourceApp.Line(Native.ProcessNameOf(targetWindow), Native.GetWindowTitle(targetWindow));
            }

            string question;
            using (var form = new AskForm(kind, preview, kind switch
            {
                AskKind.Plain => _log.History(),
                AskKind.Selection => QuickActions.All(_install.Paths.PromptsFile),
                _ => [],
            }))
            {
                if (form.ShowDialog() != DialogResult.OK)
                    return;
                question = form.Question;
            }
            _log.Remember(question);

            // The battery, a timer, a settings page: answered here, nothing sent.
            if (kind == AskKind.Plain && LocalCommands.Parse(question) is { } local)
            {
                RunLocal(question, local);
                return;
            }

            if (kind == AskKind.Screen && targetWindow != IntPtr.Zero)
            {
                Native.RestoreForeground(targetWindow);
                await FocusWait.WaitForForegroundAsync(targetWindow, Native.GetForegroundWindow);
            }

            // "search ..." and "look up ..." ask for a web search, and only they do.
            var (asked, web) = Asking.WebQuestion(question);
            var request = SourceApp.Append(Asking.BuildRequest(asked, context), appLine);
            await RunRequestAsync(kind, asked, context, request, confirm: null, answer: null, new AskOptions(Web: web, Region: area?.Arg));
        }
        finally
        {
            _asking = false;
        }
    }

    /// <summary>Asks a ready-made question (from a button) and shows the answer like any other.</summary>
    private async Task AskTextAsync(string question)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            _log.Remember(question);
            var (asked, web) = Asking.WebQuestion(question);
            await RunRequestAsync(AskKind.Plain, asked, null, Asking.BuildRequest(asked, null), confirm: null, answer: null, new AskOptions(Web: web));
        }
        finally
        {
            _asking = false;
        }
    }

    /// <summary>Does a <see cref="LocalCommand"/> and shows what it did, like any answer.</summary>
    private void RunLocal(string question, LocalCommand cmd)
    {
        string text;
        try
        {
            switch (cmd.Kind)
            {
                case LocalKind.Battery:
                    var power = SystemInformation.PowerStatus;
                    var noBattery = power.BatteryChargeStatus.HasFlag(BatteryChargeStatus.NoSystemBattery)
                        || power.BatteryLifePercent > 1;
                    text = LocalCommands.BatteryAnswer(
                        noBattery ? null : (int)Math.Round(power.BatteryLifePercent * 100),
                        power.PowerLineStatus == PowerLineStatus.Online,
                        power.BatteryChargeStatus.HasFlag(BatteryChargeStatus.Charging));
                    break;
                case LocalKind.Timer:
                    var label = cmd.Label;
                    var ui = SynchronizationContext.Current;
                    _ = Task.Delay(TimeSpan.FromSeconds(cmd.Seconds)).ContinueWith(
                        _ => ui?.Post(_ => Notify("Timer", $"{label} is up.", ToolTipIcon.Info), null),
                        TaskScheduler.Default);
                    text = LocalCommands.TimerAnswer(cmd);
                    break;
                default:
                    System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(cmd.Uri) { UseShellExecute = true });
                    text = LocalCommands.SettingsAnswer(cmd);
                    break;
            }
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or InvalidOperationException)
        {
            text = "Couldn't do that: " + e.Message;
        }
        var outcome = new AskOutcome(!text.StartsWith("Couldn't"), text, null, false);
        _log.Save(question, null, outcome, DateTime.Now);
        var answer = NewAnswerWindow(AskKind.Plain, question, "");
        answer.CanAskAnother = false;
        answer.FollowUp += () => _ = FollowUpAsync(outcome.Answer);
        ShowAnswer(answer, outcome);
        answer.Activate();
    }

    private async Task RunRequestAsync(AskKind kind, string question, string? context, string request, string? confirm, AnswerWindow? answer, AskOptions options)
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
            if (answer is null || answer.IsClosed)
                answer = NewAnswerWindow(kind, question, "Thinking…");
            else
                answer.ShowWaiting("Thinking…");

            poll.Tick += (_, _) =>
            {
                var partial = ReadPartial(request);
                if (partial != null && partial != shown && !answer.IsClosed)
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
                if (answer is null || answer.IsClosed)
                    answer = NewAnswerWindow(kind, question, "");
                answer.ShowPartial(partial);
            }, null);
        };
        var (ok, output, exitCode) = await Ctl.RunAsync(
            _install,
            Asking.CtlArguments(kind, question, request, confirm, options),
            TimeSpan.FromSeconds(240),
            onLine);
        poll.Stop();
        var outcome = Asking.Interpret(kind, question, exitCode, output);
        _log.Save(question, context, outcome, DateTime.Now);

        // A screen ask, or closed while it was on its way: show it now.
        if (answer is null || answer.IsClosed)
            answer = NewAnswerWindow(kind, question, "");

        if (outcome.Confirmation is { } nextConf)
        {
            Action<string>? onConfirm = null;
            onConfirm = action =>
            {
                if (_asking)
                {
                    Notify("FRIDAY", "Still answering. Press Run again in a moment.", ToolTipIcon.Info);
                    return;
                }
                answer.ConfirmRun -= onConfirm;
                _ = ExecuteConfirmedAsync(kind, question, context, request, action, answer, options);
            };
            answer.ConfirmRun += onConfirm;
        }

        answer.CanAskAnother = kind != AskKind.Screen;
        if (outcome.Ok)
        {
            // Another model, told to take more care and not the one that just answered.
            answer.TryAnother += () => _ = RunAgainAsync(() =>
                RunRequestAsync(kind, question, context, request, confirm: null, answer: null,
                    options with { Thorough = true, Avoid = Asking.AvoidFor(outcome.ServedBy) }));
            answer.FollowUp += () => _ = FollowUpAsync(outcome.Answer);
        }
        ShowAnswer(answer, outcome);
        answer.Activate();
    }

    private async Task RunAgainAsync(Func<Task> run)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            await run();
        }
        finally
        {
            _asking = false;
        }
    }

    private Task FollowUpAsync(string previousAnswer) => RunAgainAsync(async () =>
    {
        string question;
        var preview = previousAnswer.ReplaceLineEndings(" ");
        using (var form = new AskForm(AskKind.Plain, "Follow up on: " + (preview.Length > 50 ? preview[..50] : preview), []))
        {
            if (form.ShowDialog() != DialogResult.OK)
                return;
            question = form.Question;
        }
        _log.Remember(question);
        await RunRequestAsync(AskKind.Plain, question, null, Asking.BuildFollowUp(question, previousAnswer),
            confirm: null, answer: null, new AskOptions());
    });

    private async Task ExecuteConfirmedAsync(AskKind kind, string question, string? context, string request, string confirmAction, AnswerWindow answer, AskOptions options)
    {
        if (_asking)
            return;
        _asking = true;
        try
        {
            await RunRequestAsync(kind, question, context, request, confirmAction, answer, options);
        }
        finally
        {
            _asking = false;
        }
    }

    /// <summary>Shows the answer, and reads a good one aloud when spoken answers are on.</summary>
    private void ShowAnswer(AnswerWindow form, AskOutcome outcome)
    {
        form.ShowOutcome(outcome);
        if (outcome.Ok && outcome.Confirmation is null && !string.IsNullOrWhiteSpace(outcome.Answer)
            && Speaking.Enabled(_install.Paths))
            Ctl.Say(_install, outcome.Answer);
    }

    private AnswerWindow NewAnswerWindow(AskKind kind, string question, string waiting)
    {
        var form = new AnswerWindow(question, waiting);
        // Closing the card (or asking again) ends the reading.
        form.Closed += (_, _) => { if (Speaking.Enabled(_install.Paths)) Ctl.StopSaying(_install); };
        form.CanAskAnother = kind != AskKind.Screen;
        form.AskAgain += () => _ = AskAsync(kind);
        form.StartFriday += RestartChildren;
        FluentApp.Show(form);
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
        using var form = new ProviderForm(_install.Paths.EnvFile, () => _daemon.Restart());
        if (form.ShowDialog() == DialogResult.OK)
        {
            _daemon.Restart();
            Notify("FRIDAY", "Saved. FRIDAY restarted with the new provider.", ToolTipIcon.Info);
        }
    }

    private void ForgetSavedData()
    {
        var c = SavedData.Count(_install.Paths);
        if (!c.Any)
        {
            MessageBox.Show("FRIDAY has nothing saved on this PC.", "FRIDAY", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        var ask = $"FRIDAY keeps {c.Answers} saved answers, {c.Questions} remembered questions, " +
            $"{c.Sent} entries in the log of what was sent, and {c.Memory} things it remembered between questions.\n\n" +
            "Delete all of it? This can't be undone.";
        if (MessageBox.Show(ask, "Forget saved data", MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2) != DialogResult.Yes)
            return;
        // The daemon holds its memory open: stop it while it is deleted.
        _daemon.Stop();
        var failed = SavedData.Forget(_install.Paths);
        _daemon.Start();
        Notify("FRIDAY", failed.Count == 0 ? "Deleted." : "Couldn't delete: " + string.Join(", ", failed), failed.Count == 0 ? ToolTipIcon.Info : ToolTipIcon.Warning);
    }

    private EgressForm? _egress;

    private void ShowEgress()
    {
        if (_egress is { IsDisposed: false })
        {
            _egress.Activate();
            return;
        }
        _egress = new EgressForm(_install.Paths);
        _egress.Show();
    }

    private void ToggleAppContext()
    {
        SourceApp.SetEnabled(_install.Paths, !SourceApp.Enabled(_install.Paths));
        _appContext.Checked = SourceApp.Enabled(_install.Paths);
    }

    private bool ScreenTextOn() =>
        EnvFile.Load(_install.Paths.EnvFile).TryGetValue("FRIDAY_SCREEN_TEXT", out var v) && v is "1" or "true";

    /// <summary>
    /// Off unless the person turns it on here: a window's text can include
    /// words that are not on screen, and goes to the AI provider they use.
    /// </summary>
    private void ToggleScreenText()
    {
        var turnOn = !ScreenTextOn();
        if (turnOn && MessageBox.Show(
                "When you ask about your screen, FRIDAY can send the window's text instead of a screenshot, when the app offers enough of it.\n\n" +
                "That text can include words you can't see (scrolled out of view, in other tabs) and goes to whichever AI provider you use. Password fields are skipped and key-like secrets are masked.\n\n" +
                "Turn it on?",
                "Read a window's text", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) != DialogResult.Yes)
        {
            _screenText.Checked = false;
            return;
        }
        WriteScreenText(turnOn);
        _daemon.Restart();
        _screenText.Checked = turnOn;
        Notify("FRIDAY", turnOn ? "Screen asks may now send a window's text." : "Screen asks send one screenshot again.", ToolTipIcon.Info);
    }

    private bool _askForAdministrator;

    /// <summary>
    /// The installer's "start with administrator rights": one Windows prompt,
    /// then FRIDAY restarts the way it will start at every sign-in. Declining
    /// leaves everything as it was.
    /// </summary>
    private void AskForAdministrator()
    {
        Notify("FRIDAY", "Windows will ask once for permission to start FRIDAY as administrator.", ToolTipIcon.Info);
        var ui = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();
        Task.Run(() => Elevation.Apply(_install, true)).ContinueWith(t => ui.Post(_ =>
        {
            if (t.IsCompletedSuccessfully && t.Result)
            {
                _install.StartsAtLogin = false;
                RestartTray();
            }
            else
                Notify("FRIDAY", "Administrator start-up was not turned on. You can turn it on in Settings.", ToolTipIcon.Warning);
        }, null));
    }

    private void WriteScreenText(bool on)
    {
        var path = _install.Paths.EnvFile;
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, EnvFile.WithSetting(File.Exists(path) ? File.ReadAllText(path) : "", "FRIDAY_SCREEN_TEXT", on ? "1" : null));
        File.Move(tmp, path, overwrite: true);
    }

    /// <summary>
    /// The installer's "read a window's text" option (off unless ticked, with
    /// the same explanation as the tray's own toggle) leaves a one-shot marker
    /// in the registry; apply it once, before the daemon starts, and remove it
    /// so the tray's toggle stays the only switch afterwards.
    /// </summary>
    private void ApplyInstallerChoices()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(@"Software\FRIDAY\Options", writable: true);
            if (key?.GetValue("ScreenText") is 1)
            {
                WriteScreenText(true);
                key.DeleteValue("ScreenText");
            }
            // The administrator choice needs a Windows prompt, so it waits until
            // the tray is up (see AskForAdministrator); once is enough.
            if (key?.GetValue("AdminAccess") is 1)
            {
                key.DeleteValue("AdminAccess");
                _askForAdministrator = true;
            }
        }
        catch (Exception e)
        {
            LogUnhandledException("InstallerChoices", e);
        }
    }

    private void ToggleStartAtLogin()
    {
        _install.StartsAtLogin = !_install.StartsAtLogin;
        _startAtLogin.Checked = _install.StartsAtLogin;
    }

    private SettingsForm? _settings;

    private void OpenSettings(string? section = null)
    {
        if (_settings is { IsDisposed: false })
        {
            _settings.Activate();
            return;
        }
        _settings = new SettingsForm(_install, new SettingsActions(
            RestartDaemon: () => _daemon.Restart(),
            ChooseProvider: ChooseProvider,
            ConnectAccounts: () =>
            {
                using var dlg = new AccountsForm(_install.Paths.EnvFile, () => _daemon.Restart());
                dlg.ShowDialog();
            },
            ShowEgress: ShowEgress,
            ForgetSavedData: ForgetSavedData,
            OpenLogs: () => Open(_install.Paths.LogDir),
            OpenDataFolder: () => Open(Path.GetDirectoryName(_install.Paths.EnvFile)!),
            RestartIntoAdministrator: RestartTray,
            AskText: text => _ = string.IsNullOrWhiteSpace(text) ? AskAsync(AskKind.Plain) : AskTextAsync(text),
            ShowWelcome: ShowWelcome,
            Hotkeys: _hotkeys.Labels), section);
        _settings.Show();
    }

    /// <summary>Starts FRIDAY again the way it will start at sign-in (through its task when administrator start-up is on) and quits this one.</summary>
    private void RestartTray()
    {
        Elevation.RestartSoon(_install);
        ExitThread();
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

    private void LogUnhandledException(string source, Exception ex)
    {
        try
        {
            Directory.CreateDirectory(_install.Paths.LogDir);
            var logPath = Path.Combine(_install.Paths.LogDir, "friday-tray.log");
            File.AppendAllText(logPath, $"[{DateTime.UtcNow:O}] [{source}] {ex}{Environment.NewLine}");
        }
        catch
        {
        }
    }

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
