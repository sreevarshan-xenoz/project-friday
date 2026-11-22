using Microsoft.Win32;
using FridayTray.Core;
using FridayTray.Fluent;
using SymbolRegular = Wpf.Ui.Controls.SymbolRegular;

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
        // The windows are WPF: their exceptions go to WPF's dispatcher, not to
        // ThreadException above, and unhandled there they ended the tray.
        System.Windows.Threading.Dispatcher.CurrentDispatcher.UnhandledException += (_, e) =>
        {
            LogUnhandledException("DispatcherUnhandledException", e.Exception);
            e.Handled = true;
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

        _hotkeys.Pressed += kind => _ = kind == AskKind.Plain ? AskOrTalkAsync() : AskAsync(kind);
        _hotkeys.RegisterAll();
        _icon = new NotifyIcon { Visible = true, Text = "FRIDAY" };
        _icon.MouseUp += (_, e) => { if (e.Button == MouseButtons.Right) ShowMenu(); };
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
        WakeChanged();
        _poll.Tick += (_, _) => { UpdateStatus(); _ = RunDueTasksAsync(); CheckAgentRuns(); };
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

        _ = TellAboutUpdateAsync();
    }

    /// <summary>Once per start, a minute in: a balloon when a newer FRIDAY is out.</summary>
    private async Task TellAboutUpdateAsync()
    {
        var ui = SynchronizationContext.Current ?? new WindowsFormsSynchronizationContext();
        await Task.Delay(TimeSpan.FromMinutes(1));
        if (!AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "update-check"))
            return;
        var r = await Updater.NewerAsync();
        if (r is not null)
            ui.Post(_ => Notify($"FRIDAY {r.Version} is out", "Settings, Health & updates, Check for updates installs it.", ToolTipIcon.Info), null);
    }

    /// <summary>The tray menu, built as it opens so every check mark says what is true now.</summary>
    private void ShowMenu()
    {
        string? Key(AskKind kind) => _hotkeys.Labels.TryGetValue(kind, out var l) ? l : null;
        var paths = _install.Paths;
        TrayMenu.Open(
        [
            new("Ask FRIDAY", SymbolRegular.Sparkle24, () => _ = AskAsync(AskKind.Plain), Key(AskKind.Plain), Bold: true),
            new("Ask about selection", SymbolRegular.TextQuote24, () => _ = AskAsync(AskKind.Selection), Key(AskKind.Selection)),
            new("Ask about the screen", SymbolRegular.Desktop24, () => _ = AskAsync(AskKind.Screen), Key(AskKind.Screen)),
            new("Ask about an area…", SymbolRegular.Crop24, () => _ = AskAsync(AskKind.Screen, pickArea: true)),
            null,
            new("Last answer", SymbolRegular.ChatBubblesQuestion24, () => Open(paths.LastAnswerFile)),
            new("All answers", SymbolRegular.History24, () => Open(paths.AnswersFile)),
            new("What left this PC…", SymbolRegular.ArrowUpload24, ShowEgress),
            new("Forget saved data…", SymbolRegular.Delete24, ForgetSavedData),
            new("Name the app when asking about a selection", SymbolRegular.AppGeneric24, ToggleAppContext, Checked: SourceApp.Enabled(paths)),
            new("Read a window's text for screen asks", SymbolRegular.DocumentText24, ToggleScreenText, Checked: ScreenTextOn()),
            null,
            new("Welcome…", SymbolRegular.HandWave24, ShowWelcome),
            new("Settings…", SymbolRegular.Settings24, () => OpenSettings()),
            new("AI provider…", SymbolRegular.BrainCircuit24, ChooseProvider),
            new("Start at login", SymbolRegular.Power24, ToggleStartAtLogin, Checked: _install.StartsAtLogin),
            new("Restart FRIDAY", SymbolRegular.ArrowSync24, RestartChildren),
            new("Open logs", SymbolRegular.DocumentBulletList24, () => Open(paths.LogDir)),
            null,
            new("Quit FRIDAY", SymbolRegular.Dismiss24, ExitThread),
        ]);
    }

    private WelcomeWindow? _welcome;

    private void ShowWelcome()
    {
        if (_welcome is { IsClosed: false })
        {
            _welcome.Activate();
            return;
        }
        Welcome.MarkShown(_install.Paths);
        _welcome = new WelcomeWindow(_hotkeys.Labels, ChooseProvider, () => OpenSettings(), () => _ = AskAsync(AskKind.Plain), () => OpenSettings("Browser"));
        _welcome.Closed += (_, _) => _welcome = null;
        FluentApp.Show(_welcome);
    }

    // The request on its way now; a newer question cancels it.
    private CancellationTokenSource? _inflight;

    /// <summary>The newest question wins: an answer still on its way is cancelled.</summary>
    private async Task ReplaceInflightAsync()
    {
        if (!_asking || _inflight is not { } current)
            return;
        current.Cancel();
        for (var i = 0; i < 40 && _asking; i++)
            await Task.Delay(50);
    }

    private async Task AskAsync(AskKind kind, bool pickArea = false)
    {
        await ReplaceInflightAsync();
        if (_asking)
            return;
        _asking = true;
        try
        {
            // The window in front, read before anything of ours opens. "Ask
            // again" from an answer card means the window asked about before;
            // the taskbar or tray is no window to look at or type into.
            var targetWindow = Native.GetForegroundWindow();
            if (string.Equals(Native.ProcessNameOf(targetWindow), "friday-tray", StringComparison.OrdinalIgnoreCase))
                targetWindow = _askTarget;
            if (Native.GetClassName(targetWindow) is "Shell_TrayWnd" or "NotifyIconOverflowWindow" or "Shell_SecondaryTrayWnd")
                targetWindow = IntPtr.Zero;
            _askTarget = targetWindow;
            // Drag out the area first: only that part of the screen is captured.
            PickedRegion? area = null;
            if (pickArea)
            {
                using var picker = new RegionForm();
                if (picker.ShowDialog() != DialogResult.OK)
                    return;
                area = picker.Picked;
            }
            // A password manager, a private browser window, a bank: neither a
            // screenshot of it nor text selected in it is sent anywhere.
            if (kind is AskKind.Screen or AskKind.Selection
                && PrivateWindows.Match(_install.Paths, Native.ProcessNameOf(targetWindow), Native.GetWindowTitle(targetWindow)) is { } privateBy)
            {
                Notify("FRIDAY", $"This window looks private (\"{privateBy}\"), so nothing from it was sent. " +
                    $"Your own list: {_install.Paths.PrivateWindowsFile}", ToolTipIcon.Info);
                return;
            }
            // A mail or chat app: the box says its text, mostly someone
            // else's words, goes to the AI provider.
            var sensitiveBy = kind is AskKind.Screen or AskKind.Selection
                && AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "sensitive-warn")
                ? SensitiveWindows.Match(_install.Paths, Native.ProcessNameOf(targetWindow), Native.GetWindowTitle(targetWindow))
                : null;
            string? context = null;
            string? preview = null;
            string? appLine = null;
            var selected = false;
            if (kind == AskKind.Selection)
            {
                var sel = await Selection.CaptureAsync();
                if (sel.Problem != null)
                {
                    Notify("FRIDAY", sel.Problem, ToolTipIcon.Info);
                    return;
                }
                context = sel.Text!.Length > Asking.MaxContextChars ? sel.Text[..Asking.MaxContextChars] : sel.Text;
                selected = sel.Source == "Selected";
                preview = $"{sel.Source}: {context.ReplaceLineEndings(" ")}";
                preview = preview.Length > 60 ? preview[..60] : preview;
                // Read now, while the window the text came from is still in front.
                if (SourceApp.Enabled(_install.Paths))
                    appLine = SourceApp.Line(Native.ProcessNameOf(targetWindow), Native.GetWindowTitle(targetWindow));
            }

            var form = new AskWindow(kind, preview, sensitiveBy, kind switch
            {
                AskKind.Plain => _log.History(),
                AskKind.Selection => QuickActions.All(_install.Paths.PromptsFile, selected),
                _ => [],
            })
            {
                Folders = kind == AskKind.Plain && AppSettings.AgentsOn(EnvFile.Load(_install.Paths.EnvFile))
                    ? AgentFolders.Load(_install.Paths.AgentsDir, Environment.GetFolderPath(Environment.SpecialFolder.UserProfile))
                    : null,
            };
            if (form.ShowDialog() != true)
                return;
            var question = form.Question;
            _log.Remember(question);

            // "Rewrite: …" on selected text: the rewrite replaces the selection.
            if (selected && context != null && Rewrite.How(question) is { } how)
            {
                await RewriteAsync(targetWindow, question, how, context);
                return;
            }

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

            // A browser in front with FRIDAY's extension: the page's text
            // instead of a screenshot (an area dragged out stays a screenshot).
            if (kind == AskKind.Screen && area is null && BrowserLink.All.Any(b =>
                    string.Equals(Path.GetFileNameWithoutExtension(b.Exe), Native.ProcessNameOf(targetWindow), StringComparison.OrdinalIgnoreCase)))
            {
                var tab = await Ctl.RunAsync(_install, ["browser", "tab"], TimeSpan.FromSeconds(8));
                if (tab.Ok && BrowserLink.PageContext(tab.Output, Native.GetWindowTitle(targetWindow) ?? "") is { } page)
                {
                    kind = AskKind.Plain;
                    context = page;
                }
            }

            // "search ..." and "look up ..." ask for a web search, and only they do.
            var (asked, web) = Asking.WebQuestion(question);
            var request = SourceApp.Append(Asking.BuildRequest(asked, context), appLine);
            // "How do I … in FRIDAY?": FRIDAY's guide goes with the question.
            if (kind == AskKind.Plain && context is null && !web && Asking.AboutFriday(asked))
            {
                var docs = await Ctl.RunAsync(_install, ["docs", "--", asked], TimeSpan.FromSeconds(3));
                if (docs.Ok)
                    request = Asking.WithDocs(request, docs.Output);
            }
            await RunRequestAsync(kind, asked, context, request, confirm: null, answer: null, new AskOptions(Web: web, Region: area?.Arg));
        }
        finally
        {
            _asking = false;
        }
    }

    /// <summary>
    /// Asks for the rewrite and pastes it over the selection; into a terminal
    /// (whose selection can't be replaced) or a window that went away, it is
    /// left on the clipboard instead.
    /// </summary>
    private async Task RewriteAsync(IntPtr target, string question, string how, string text)
    {
        Notify("FRIDAY", $"Rewriting: {how}…", ToolTipIcon.Info);
        var r = await Ctl.RunAsync(_install, Asking.CtlArguments(AskKind.Plain, question, Rewrite.Request(how, text)), TimeSpan.FromMinutes(3));
        var outcome = Asking.Interpret(AskKind.Plain, question, r.ExitCode, r.Output);
        var rewritten = outcome.Ok ? Rewrite.Clean(outcome.Answer) : "";
        if (rewritten.Length == 0)
        {
            Notify("FRIDAY", "Couldn't rewrite it: " + outcome.Answer, ToolTipIcon.Warning);
            return;
        }
        if (Terminals.IsTerminalClass(Native.GetClassName(target)) || !await Selection.PasteAsync(target, rewritten))
            Notify("FRIDAY", Selection.TrySetText(rewritten)
                ? "The rewrite is on your clipboard: paste it with Ctrl+V."
                : "The clipboard is busy, so the rewrite couldn't be put there.", ToolTipIcon.Info);
    }

    /// <summary>
    /// Asks what the daemon queued from a standing task, one goal at a time,
    /// the way the person's own ask runs (so it gets the same answer window,
    /// questions before anything risky, history and egress log). Waits while
    /// another ask is on screen.
    /// </summary>
    private async Task RunDueTasksAsync()
    {
        if (_asking || _runningDue)
            return;
        _runningDue = true;
        try
        {
            foreach (var goal in StandingTasks.TakeDue(_install.Paths.StandingDueFile))
            {
                while (_asking)
                    await Task.Delay(1000);
                await AskTextAsync(goal);
            }
        }
        catch (IOException e)
        {
            Notify("FRIDAY", "Couldn't run a standing task: " + e.Message, ToolTipIcon.Warning);
        }
        finally
        {
            _runningDue = false;
        }
    }

    private bool _runningDue;
    private readonly HashSet<string> _knownFinishedRunIds = new(StringComparer.Ordinal);
    /// <summary>The step each waiting run last showed, so a new one opens a new window.</summary>
    private readonly Dictionary<string, string> _shownStep = new(StringComparer.Ordinal);
    private readonly Dictionary<string, AgentPromptWindow> _waitingWindows = new(StringComparer.Ordinal);
    private bool _agentsInitialized;

    /// <summary>
    /// Checks for agent runs waiting for person approval or questions, and
    /// notifies when an agent run finishes.
    /// </summary>
    private void CheckAgentRuns()
    {
        var agentsDir = _install.Paths.AgentsDir;
        var runs = AgentRuns.Load(agentsDir);
        if (!_agentsInitialized)
        {
            // Runs that ended before the tray started aren't announced again.
            foreach (var run in runs.Where(r => r.IsFinished))
                _knownFinishedRunIds.Add(run.Id);
            _agentsInitialized = true;
        }

        foreach (var run in runs)
        {
            if (run.IsWaiting)
            {
                // A second step can follow the first between two polls, so
                // what counts is the step, not just "waiting".
                var step = (run.Pending?.Step ?? "") + "\n" + (run.Pending?.Why ?? "");
                if (!_shownStep.TryGetValue(run.Id, out var shown) || shown != step)
                {
                    _shownStep[run.Id] = step;
                    if (_waitingWindows.Remove(run.Id, out var old) && !old.IsClosed)
                        old.Close();
                    OpenWaitingWindow(agentsDir, run);
                }
            }
            else
            {
                if (_waitingWindows.TryGetValue(run.Id, out var openWin))
                {
                    if (!openWin.IsClosed)
                        openWin.Close();
                    _waitingWindows.Remove(run.Id);
                }
            }

            if (run.Status is "done" or "failed")
            {
                if (_knownFinishedRunIds.Add(run.Id))
                {
                    var folderName = "";
                    if (!string.IsNullOrEmpty(run.Folder))
                    {
                        folderName = Path.GetFileName(run.Folder.TrimEnd('\\', '/'));
                        if (string.IsNullOrEmpty(folderName))
                            folderName = run.Folder;
                    }
                    var firstLine = "";
                    if (!string.IsNullOrEmpty(run.Summary))
                        firstLine = run.Summary.Split('\n', 2)[0].TrimEnd('\r');
                    var text = string.IsNullOrWhiteSpace(firstLine) ? "Done." : firstLine;
                    var ended = run.Status == "failed" ? "stopped" : "finished";
                    Notify($"{run.Agent} {ended} in {folderName}", text,
                        run.Status == "failed" ? ToolTipIcon.Warning : ToolTipIcon.Info);
                }
            }
        }
    }

    private void OpenWaitingWindow(string agentsDir, AgentRun run)
    {
        if (_waitingWindows.TryGetValue(run.Id, out var existing) && !existing.IsClosed)
        {
            existing.Activate();
            return;
        }
        var win = new AgentPromptWindow(agentsDir, run);
        win.Closed += (_, _) => _waitingWindows.Remove(run.Id);
        _waitingWindows[run.Id] = win;
        FluentApp.Show(win);
    }

    /// <summary>Asks a ready-made question (from a button) and shows the answer like any other.</summary>
    /// <summary>
    /// The ask key: a tap opens the ask box; held (with voice set up), it
    /// listens until let go and asks what was said.
    /// </summary>
    private async Task AskOrTalkAsync()
    {
        // The key stops FRIDAY talking at once, tap or hold, and ends a wait
        // for a spoken follow-up or run/cancel (the key is the answer now).
        if (Speaking.Enabled(_install.Paths))
            Ctl.StopSaying(_install);
        try { if (_listener is { HasExited: false } waiting) waiting.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
        await ReplaceInflightAsync();
        if (_asking || !Listening.Installed(_install.Paths)
            || !AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "hold-to-talk"))
        {
            await AskAsync(AskKind.Plain);
            return;
        }
        await Task.Delay(Listening.HoldDelay);
        if (!YHeld())
        {
            await AskAsync(AskKind.Plain);
            return;
        }
        var key = _hotkeys.Labels.TryGetValue(AskKind.Plain, out var l) ? l : "the ask key";
        string text;
        int code;
        _asking = true;
        try
        {
            _icon.Text = "FRIDAY: listening…";
            var psi = new System.Diagnostics.ProcessStartInfo(_install.Ctl)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = System.Text.Encoding.UTF8,
            };
            psi.ArgumentList.Add("listen");
            psi.ArgumentList.Add("hold");
            foreach (var (k, v) in _install.ChildEnvironment())
                psi.Environment[k] = v;
            using var p = System.Diagnostics.Process.Start(psi);
            if (p is null)
                return;
            var output = p.StandardOutput.ReadToEndAsync();
            _ = p.StandardError.ReadToEndAsync();
            // friday-ctl stops by itself after 30 s.
            while (YHeld() && !p.HasExited)
                await Task.Delay(30);
            try { p.StandardInput.Close(); } catch (IOException) { }
            _icon.Text = "FRIDAY: working out what you said…";
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(90));
            try
            {
                await p.WaitForExitAsync(cts.Token);
            }
            catch (OperationCanceledException)
            {
                try { p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            }
            text = (await output).Trim();
            code = p.HasExited ? p.ExitCode : -1;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException)
        {
            text = "";
            code = -1;
        }
        finally
        {
            _asking = false;
            UpdateStatus();
        }
        // Let go almost at once: that was a tap after all.
        if (code == 5)
        {
            await AskAsync(AskKind.Plain);
            return;
        }
        if (code != 0 || text.Length == 0)
        {
            Notify("FRIDAY", Listening.Problem(code == 0 ? 1 : code, key)!, ToolTipIcon.Info);
            return;
        }
        // Spoken like typed: "set a timer for 5 minutes" is answered here too.
        if (LocalCommands.Parse(text) is { } local)
        {
            _log.Remember(text);
            RunLocal(text, local);
            return;
        }
        await AskTextAsync(text, spoken: true);
    }

    private static bool YHeld() => (Native.GetAsyncKeyState('Y') & 0x8000) != 0;

    private async Task AskTextAsync(string question, bool spoken = false)
    {
        if (_asking)
            return;
        _asking = true;
        // Asked from the tray or a standing task: no window to type into.
        _askTarget = IntPtr.Zero;
        try
        {
            _log.Remember(question);
            var (asked, web) = Asking.WebQuestion(question);
            await RunRequestAsync(AskKind.Plain, asked, null, Asking.BuildRequest(asked, null), confirm: null, answer: null, new AskOptions(Web: web, Spoken: spoken));
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
        using var inflight = new CancellationTokenSource();
        _inflight = inflight;
        var ui = SynchronizationContext.Current;
        Action<string>? onLine = kind != AskKind.Screen || ui is null ? null : line =>
        {
            if (Asking.PartialLine(line) is not { } partial)
                return;
            ui.Post(_ =>
            {
                // Replaced by a newer question: its late lines open nothing.
                if (inflight.IsCancellationRequested)
                    return;
                if (answer is null || answer.IsClosed)
                    answer = NewAnswerWindow(kind, question, "");
                answer.ShowPartial(partial);
            }, null);
        };
        var (ok, output, exitCode) = await Ctl.RunAsync(
            _install,
            Asking.CtlArguments(kind, question, request, confirm, options),
            TimeSpan.FromSeconds(240),
            onLine,
            inflight.Token);
        if (_inflight == inflight)
            _inflight = null;
        poll.Stop();
        // A newer question replaced this one: its card goes, nothing is shown.
        if (inflight.IsCancellationRequested)
        {
            if (answer is { IsClosed: false })
                answer.Close();
            return;
        }
        var outcome = Asking.Interpret(kind, question, exitCode, output);
        _log.Save(question, context, outcome, DateTime.Now);

        // A screen ask, or closed while it was on its way: show it now.
        if (answer is null || answer.IsClosed)
            answer = NewAnswerWindow(kind, question, "");

        // "Act, then tell me" (Settings, Desktop control): what FRIDAY would
        // have asked about runs now, once, and a balloon says what it did. The
        // daemon still runs only that exact action.
        if (outcome.Confirmation is { } autoConf
            && AppSettings.AutoConfirm(AppSettings.AutonomyMode(EnvFile.Load(_install.Paths.EnvFile)),
                screenAsk: kind == AskKind.Screen, alreadyConfirming: confirm is not null))
        {
            Notify("FRIDAY acted on its own", outcome.Answer + "\n(Settings, Desktop control: How much FRIDAY may do)", ToolTipIcon.Info);
            await RunRequestAsync(kind, question, context, request, autoConf.Action, answer, options);
            return;
        }
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
        ShowAnswer(answer, outcome, options.Spoken);
        answer.Activate();
        if (outcome.Confirmation is { } spokenConf && options.Spoken && Listening.Installed(_install.Paths)
            && AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "voice-confirm"))
            _ = ListenForAnswerAsync(answer, spokenConf.Action);
    }

    /// <summary>
    /// Asked by voice: after a sound, a spoken "run" or "cancel" answers the
    /// step on screen, heard while it is there only. What can't be undone is
    /// cancelled by voice but needs a click to run.
    /// </summary>
    private async Task ListenForAnswerAsync(AnswerWindow answer, string action)
    {
        var irreversible = Listening.Irreversible(action);
        Ctl.StopSaying(_install);
        var said = Listening.YesNo(await ListenOnceAsync(answer,
            irreversible ? "After the sound, say cancel. Run needs a click." : "After the sound, say run or cancel."));
        if (answer.Waiting && (said == "cancel" || (said == "run" && !irreversible)))
            answer.AnswerByVoice(said == "run");
    }

    /// <summary>
    /// Asked by voice and read aloud to the end (not stopped, the card still
    /// open): after a sound, a follow-up needs no key.
    /// </summary>
    private async Task SayThenListenAsync(AnswerWindow answer, string said)
    {
        // With "Hey Friday" on, its own conversation hears the follow-up.
        if (await Ctl.SayAsync(_install, said) != 0 || answer.IsClosed || _wake is not null)
            return;
        var heard = await ListenOnceAsync(answer, "After the sound, ask a follow-up, or say stop.");
        if (string.IsNullOrWhiteSpace(heard) || Listening.EndsConversation(heard))
            return;
        await RunAgainAsync(() =>
        {
            _log.Remember(heard);
            return RunRequestAsync(AskKind.Plain, heard, null, Asking.BuildFollowUp(heard, said),
                confirm: null, answer: null, new AskOptions(Spoken: true));
        });
    }

    // The voice listener waiting now, if any (the ask key ends it).
    private System.Diagnostics.Process? _listener;

    /// <summary>
    /// One short spoken answer while <paramref name="answer"/> is on screen
    /// (`friday-ctl listen answer`: a sound when it listens, about six
    /// seconds); null when nothing was said or the card closed.
    /// </summary>
    private async Task<string?> ListenOnceAsync(AnswerWindow answer, string hint)
    {
        var psi = new System.Diagnostics.ProcessStartInfo(_install.Ctl)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = System.Text.Encoding.UTF8,
        };
        psi.ArgumentList.Add("listen");
        psi.ArgumentList.Add("answer");
        foreach (var (k, v) in _install.ChildEnvironment())
            psi.Environment[k] = v;
        System.Diagnostics.Process? p;
        try { p = System.Diagnostics.Process.Start(psi); }
        catch (System.ComponentModel.Win32Exception) { return null; }
        if (p is null)
            return null;
        _listener = p;
        void Stop()
        {
            try { if (!p.HasExited) p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
        }
        // Answered by a click (Run goes on to work in the same card) or closed:
        // the microphone closes too.
        EventHandler closed = (_, _) => Stop();
        Action<string> clicked = _ => Stop();
        answer.Closed += closed;
        answer.ConfirmRun += clicked;
        answer.ShowVoiceHint(hint);
        try
        {
            _ = p.StandardError.ReadToEndAsync();
            while (await p.StandardOutput.ReadLineAsync() is { } line)
            {
                if (line != "listening")
                    return answer.IsClosed ? null : line;
                if (!answer.IsClosed)
                    System.Media.SystemSounds.Asterisk.Play();
            }
            return null;
        }
        catch (IOException)
        {
            return null;
        }
        finally
        {
            answer.Closed -= closed;
            answer.ConfirmRun -= clicked;
            Stop();
            if (_listener == p)
                _listener = null;
            p.Dispose();
        }
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
        var form = new AskWindow(AskKind.Plain, "Follow up on: " + (preview.Length > 50 ? preview[..50] : preview), null, []);
        if (form.ShowDialog() != true)
            return;
        question = form.Question;
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

    /// <summary>
    /// Shows the answer, and reads a good one aloud when spoken answers are
    /// on; asked by voice, a follow-up can be spoken after it.
    /// </summary>
    private void ShowAnswer(AnswerWindow form, AskOutcome outcome, bool spoken = false)
    {
        form.ShowOutcome(outcome);
        if (!outcome.Ok || outcome.Confirmation is not null || string.IsNullOrWhiteSpace(outcome.Answer)
            || !Speaking.Enabled(_install.Paths))
            return;
        if (spoken && Listening.Installed(_install.Paths)
            && AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "voice-followup"))
            _ = SayThenListenAsync(form, outcome.Answer);
        else
            Ctl.Say(_install, outcome.Answer);
    }

    private AnswerWindow NewAnswerWindow(AskKind kind, string question, string waiting)
    {
        var form = new AnswerWindow(question, waiting);
        // Closing the card (or asking again) ends the reading.
        form.Closed += (_, _) => { if (Speaking.Enabled(_install.Paths)) Ctl.StopSaying(_install); };
        form.CanAskAnother = kind != AskKind.Screen;
        form.AskAgain += () => _ = AskAsync(kind);
        // "That was wrong" shows only while its switch is on (Settings, General).
        if (AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "wrong-button"))
        {
            form.Wrong += outcome =>
            {
                try
                {
                    var version = typeof(TrayContext).Assembly.GetName().Version?.ToString(3) ?? "";
                    Repro.Save(_install.Paths, kind, outcome, question.Length, "Wrong", version);
                }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
            };
        }
        // This card's own window: a later ask doesn't move where it types.
        var target = _askTarget;
        form.TypeIt += text => _ = TypeItAsync(target, text);
        form.StartFriday += RestartChildren;
        FluentApp.Show(form);
        form.Activate();
        return form;
    }

    /// <summary>The window the last question was asked from ("Type it" puts the answer there).</summary>
    private IntPtr _askTarget;

    private async Task TypeItAsync(IntPtr target, string text)
    {
        if (target != IntPtr.Zero && await Selection.PasteAsync(target, text))
            return;
        Notify("FRIDAY", Selection.TrySetText(text)
            ? "The answer is on your clipboard: paste it with Ctrl+V."
            : "The clipboard is busy, so the answer couldn't be put there. Use Copy on the answer.", ToolTipIcon.Info);
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
        var text = _wake is null ? state.Detail : "Hey Friday: listening · " + state.Detail;
        _icon.Text = text.Length > 60 ? text[..60] : text;
        if (state.Mode == _mode)
            return;
        _mode = state.Mode;
        var old = _icon.Icon;
        _icon.Icon = Theme.Orb(state.Mode);
        old?.Dispose();
    }

    private void ChooseProvider()
    {
        var form = new ProviderWindow(_install.Paths.EnvFile, () => _daemon.Restart());
        // Owned by Settings when it's open: the dialog stays above it, and
        // minimizing or restoring Settings brings it along (an unowned modal
        // dialog could end up hidden while Settings stays blocked).
        if (_settings is { IsClosed: false } owner)
            form.Owner = owner;
        if (form.ShowDialog() == true)
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

    private EgressWindow? _egress;

    private void ShowEgress()
    {
        if (_egress is { IsClosed: false })
        {
            _egress.Activate();
            return;
        }
        _egress = new EgressWindow(_install.Paths);
        FluentApp.Show(_egress);
    }

    private void ToggleAppContext()
    {
        SourceApp.SetEnabled(_install.Paths, !SourceApp.Enabled(_install.Paths));
    }

    private bool ScreenTextOn() => AppSettings.ScreenTextOn(EnvFile.Load(_install.Paths.EnvFile));

    /// <summary>
    /// On by default: a screen ask sends the window's text when the app
    /// offers enough of it (faster, and less than a screenshot). It can
    /// include words that are not on screen, so it can be turned off here.
    /// </summary>
    private void ToggleScreenText()
    {
        var turnOn = !ScreenTextOn();
        WriteScreenText(turnOn);
        _daemon.Restart();
        Notify("FRIDAY", turnOn ? "Screen asks send a window's text when the app offers it." : "Screen asks send one screenshot.", ToolTipIcon.Info);
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
        File.WriteAllText(tmp, AppSettings.WithScreenText(File.Exists(path) ? File.ReadAllText(path) : "", on));
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
            // Screen text is on by default: the installer ran without it ticked means off.
            if (key?.GetValue("ScreenText") is 1)
            {
                WriteScreenText(true);
                key.DeleteValue("ScreenText");
            }
            else if (key?.GetValue("ScreenTextAsked") is 1)
                WriteScreenText(false);
            if (key?.GetValue("ScreenTextAsked") is 1)
                key.DeleteValue("ScreenTextAsked");
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
    }

    private SettingsWindow? _settings;

    private void OpenSettings(string? section = null)
    {
        if (_settings is { IsClosed: false })
        {
            if (section is not null)
                _settings.Show(section);
            _settings.Activate();
            return;
        }
        _settings = new SettingsWindow(_install, new SettingsActions(
            RestartDaemon: () => _daemon.Restart(),
            ChooseProvider: ChooseProvider,
            ConnectAccounts: () =>
            {
                var accounts = new AccountsWindow(_install.Paths.EnvFile, () => _daemon.Restart());
                if (_settings is { IsClosed: false } owner)
                    accounts.Owner = owner;
                accounts.ShowDialog();
            },
            ShowEgress: ShowEgress,
            ForgetSavedData: ForgetSavedData,
            OpenLogs: () => Open(_install.Paths.LogDir),
            OpenDataFolder: () => Open(Path.GetDirectoryName(_install.Paths.EnvFile)!),
            RestartIntoAdministrator: RestartTray,
            AskText: text => _ = string.IsNullOrWhiteSpace(text) ? AskAsync(AskKind.Plain) : AskTextAsync(text),
            ShowWelcome: ShowWelcome,
            WakeChanged: WakeChanged,
            Hotkeys: _hotkeys.Labels), section);
        FluentApp.Show(_settings);
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

    // "Hey Friday" (Settings, Voice; off unless turned on): friday-ctl listens
    // on this PC and says what it heard; the tray asks it. Restarted if it
    // stops, until switched off.
    private CancellationTokenSource? _wake;

    private void WakeChanged()
    {
        _wake?.Cancel();
        _wake = null;
        if (Listening.WakeOn(_install.Paths))
        {
            _wake = new CancellationTokenSource();
            _ = WakeLoopAsync(_wake.Token);
        }
        UpdateStatus();
    }

    private async Task WakeLoopAsync(CancellationToken stop)
    {
        while (!stop.IsCancellationRequested)
        {
            var psi = new System.Diagnostics.ProcessStartInfo(_install.Ctl)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = System.Text.Encoding.UTF8,
            };
            psi.ArgumentList.Add("listen");
            psi.ArgumentList.Add("wake");
            foreach (var (k, v) in _install.ChildEnvironment())
                psi.Environment[k] = v;
            try
            {
                using var p = System.Diagnostics.Process.Start(psi);
                if (p is null)
                    return;
                using var kill = stop.Register(() => { try { p.Kill(); } catch (InvalidOperationException) { } });
                _ = p.StandardError.ReadToEndAsync();
                while (await p.StandardOutput.ReadLineAsync() is { } line)
                {
                    if (stop.IsCancellationRequested)
                        break;
                    if (Listening.WakeLine(line) is not { } heard)
                        continue;
                    var (kind, text) = heard;
                    if (kind != "note")
                        await ReplaceInflightAsync();
                    if (kind == "note")
                        Notify("FRIDAY", text, ToolTipIcon.None);
                    else if (_asking)
                        Notify("FRIDAY", $"Still answering, so this wasn't asked: \"{text}\". Ask again in a moment.", ToolTipIcon.Info);
                    else if (kind == "ask")
                        _ = AskTextAsync(text);
                    else
                        _ = WakeFollowUpAsync(text);
                }
            }
            catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException)
            {
            }
            try { await Task.Delay(TimeSpan.FromSeconds(5), stop); }
            catch (TaskCanceledException) { return; }
        }
    }

    /// <summary>In a hands-free conversation: the question follows on from the last answer.</summary>
    private Task WakeFollowUpAsync(string question)
    {
        string last;
        try { last = File.ReadAllText(_install.Paths.LastAnswerFile); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return AskTextAsync(question); }
        return RunAgainAsync(() =>
        {
            _log.Remember(question);
            return RunRequestAsync(AskKind.Plain, question, null, Asking.BuildFollowUp(question, last),
                confirm: null, answer: null, new AskOptions());
        });
    }

    protected override void ExitThreadCore()
    {
        _wake?.Cancel();
        _poll.Stop();
        _hotkeys.Dispose();
        _daemon.Dispose();
        _host.Dispose();
        _icon.Visible = false;
        _icon.Dispose();
        base.ExitThreadCore();
    }
}
