using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Animation;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Button = System.Windows.Controls.Button;
using Color = System.Windows.Media.Color;
using ComboBox = System.Windows.Controls.ComboBox;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using Brushes = System.Windows.Media.Brushes;
using FontFamily = System.Windows.Media.FontFamily;
using MessageBox = System.Windows.MessageBox;
using MessageBoxButton = System.Windows.MessageBoxButton;
using MessageBoxResult = System.Windows.MessageBoxResult;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

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
    Action WakeChanged,
    Action LocalAiChanged,
    Action StopLlama,
    Func<TimeSpan> LlamaUptime,
    IReadOnlyDictionary<AskKind, string> Hotkeys);

/// <summary>
/// FRIDAY's settings: sections on the left, cards on the right. A switch takes
/// effect as soon as it is flipped (friday.env is rewritten and the daemon
/// restarted when that is what it changes); nothing here is a second copy of a
/// setting, so the tray menu and friday.env always agree with it.
/// </summary>
internal sealed partial class SettingsWindow : FluentWindow
{
    private static readonly (string Name, SymbolRegular Icon)[] Sections =
    [
        ("Home", SymbolRegular.Home24),
        ("General", SymbolRegular.Settings24),
        ("Privacy", SymbolRegular.ShieldLock24),
        ("AI", SymbolRegular.BrainCircuit24),
        ("Local AI", SymbolRegular.HardDrive24),
        ("Voice", SymbolRegular.Speaker224),
        ("Desktop control", SymbolRegular.Cursor24),
        ("Agents", SymbolRegular.Bot24),
        ("Tasks", SymbolRegular.CalendarClock24),
        ("Shortcuts", SymbolRegular.Keyboard24),
        ("Browser", SymbolRegular.Globe24),
        ("Health & updates", SymbolRegular.HeartPulse24),
        ("About", SymbolRegular.Info24),
    ];

    private readonly Install _install;
    private readonly SettingsActions _do;
    private readonly Dictionary<string, Button> _nav = new();
    private string _current = "";

    /// <summary>Closed; the tray makes a new one when asked again.</summary>
    public bool IsClosed { get; private set; }

    public SettingsWindow(Install install, SettingsActions actions, string? section = null)
    {
        InitializeComponent();
        _install = install;
        _do = actions;
        foreach (var (name, icon) in Sections)
        {
            var b = NavButton(name, icon);
            _nav[name] = b;
            Nav.Children.Add(b);
        }
        Closed += (_, _) => IsClosed = true;
        // Every card as wide as the page, up to a comfortable reading width.
        Scroll.SizeChanged += (_, _) => Page.Width = Math.Max(300, Math.Min(760, Scroll.ActualWidth - 72));
        Loaded += (_, _) =>
        {
            Show(section is not null && _nav.ContainsKey(section) ? section : "Home", animate: false);
            _nav[_current].Focus(); // the keyboard starts in the sidebar (arrows move, Enter opens)
        };
    }

    /// <summary>Opens <paramref name="name"/>: the marker glides to it and its cards rise in.</summary>
    public void Show(string name, bool animate = true)
    {
        var index = Array.FindIndex(Sections, s => s.Name == name);
        if (index < 0)
            return;
        var y = index * 38.0;
        if (animate)
            MarkerMove.BeginAnimation(TranslateTransform.YProperty,
                new DoubleAnimation(y, TimeSpan.FromMilliseconds(460)) { EasingFunction = new Spring { Damping = 0.7 } });
        else
        {
            MarkerMove.BeginAnimation(TranslateTransform.YProperty, null);
            MarkerMove.Y = y;
        }
        foreach (var (n, b) in _nav)
            b.Foreground = Resource(n == name ? "Text" : "Subtext");

        var same = name == _current;
        _current = name;
        Page.Children.Clear();
        var title = new TextBlock
        {
            Text = name,
            FontFamily = (FontFamily)FindResource("DisplayFont"),
            FontSize = 26,
            FontWeight = FontWeights.SemiBold,
            Foreground = Resource("Text"),
            Margin = new Thickness(0, 0, 0, 18),
        };
        Page.Children.Add(title);
        foreach (var e in Group(name switch
        {
            "Home" => Home(),
            "General" => General(),
            "Privacy" => Privacy(),
            "AI" => Ai(),
            "Local AI" => LocalAiSection(),
            "Voice" => Voice(),
            "Desktop control" => Desktop(),
            "Agents" => Agents(),
            "Tasks" => Tasks(),
            "Shortcuts" => Shortcuts(),
            "Browser" => Browser(),
            "Health & updates" => HealthAndUpdates(),
            _ => About(),
        }))
            Page.Children.Add(e);
        if (!same)
        {
            Scroll.ScrollToTop();
            for (var i = 0; i < Page.Children.Count; i++)
                Motion.RiseIn(Page.Children[i], dy: 12, delayMs: Math.Min(i, 8) * 30, ms: 460);
        }
    }

    // ---- pages ----

    private IEnumerable<UIElement> Home()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        yield return Pair("FRIDAY", DaemonState.Read(_install.Paths.StateFile).Detail);
        yield return Pair("AI", AppSettings.UsingFreeModels(env)
            ? "Free models (they may keep what you send)"
            : "Your own account or key");
        yield return Pair("Voice", !Speaking.Installed(_install.Paths) ? "Not set up"
            : Speaking.Enabled(_install.Paths) ? "On" : "Off");
        yield return Note("Press " + (_do.Hotkeys.TryGetValue(AskKind.Plain, out var k) ? k : "the tray icon") + " any time to ask FRIDAY anything.");
        yield return Row(Pill("Ask FRIDAY something", SymbolRegular.Sparkle24, primary: true), () => _do.AskText(""));
        yield return Row(Pill("How is my computer doing?", SymbolRegular.HeartPulse24), () => _do.AskText("How is my computer doing?"));
        yield return Row(Pill("Show the welcome again", SymbolRegular.HandWave24), _do.ShowWelcome);
    }

    private IEnumerable<UIElement> Browser()
    {
        yield return Note("With FRIDAY's extension in your browser, asking about your screen while a page is in front sends the page's text (title, address, selection) instead of a screenshot. " +
            "The browser starts FRIDAY itself and talks to it over a private pipe: no port, nothing from the web can reach it. It reads the tab only when you ask, and never in a private window.");
        var any = false;
        foreach (var b in BrowserSetup.Installed())
        {
            any = true;
            var browser = b;
            yield return Pair(b.Name, BrowserSetup.Connected(b) ? "Connected" : "Not connected");
            yield return Row(Pill($"Connect {b.Name}", SymbolRegular.PlugConnected24, primary: !BrowserSetup.Connected(b)), () =>
            {
                var steps = BrowserSetup.Connect(browser, _install);
                MessageBox.Show(this, steps, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Information);
                Show("Browser");
            });
        }
        if (!any)
            yield return Note("No browser FRIDAY knows was found (Chrome, Edge, Brave, Vivaldi, Firefox).");
    }

    private IEnumerable<UIElement> HealthAndUpdates()
    {
        yield return Pair("This version", Version());
        yield return Note("When you sign in, FRIDAY asks GitHub whether a newer version is out. Updating downloads the new installer, checks it against the release's checksum and opens it; your settings and data are kept.");
        yield return Row(Pill("Check for updates", SymbolRegular.ArrowDownload24, primary: true), CheckForUpdates);
        yield return Row(Pill("Check my computer's health", SymbolRegular.HeartPulse24), () => _do.AskText("How is my computer doing?"));
        yield return Row(Pill("Restart FRIDAY", SymbolRegular.ArrowSync24), () => { _do.RestartDaemon(); Said("FRIDAY restarted."); });
        yield return Note("Something wrong? Save a short report to your desktop to send to whoever is helping. It has versions and which switches are on; no questions, answers, logs or keys.");
        yield return Row(Pill("Save a support report", SymbolRegular.DocumentSave24), SaveReport);
    }

    private async void CheckForUpdates()
    {
        // async void: anything unexpected is shown here instead of ending the tray.
        try
        {
            await CheckForUpdatesAsync();
        }
        catch (Exception e)
        {
            MessageBox.Show(this, "FRIDAY wasn't updated:\n\n" + e.Message, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private async Task CheckForUpdatesAsync()
    {
        Said("Checking for a newer FRIDAY…");
        var r = await Updater.NewerAsync();
        if (r is null)
        {
            Said($"You have the newest FRIDAY ({Version()}), or GitHub couldn't be reached.");
            return;
        }
        if (MessageBox.Show(this, $"FRIDAY {r.Version} is out (you have {Version()}). Download and install it now?\n\nFRIDAY closes while the installer runs and starts again when it finishes.",
                "FRIDAY", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes)
            return;
        Said($"Downloading FRIDAY {r.Version}…");
        var keep = AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "update-rollback") ? _install.Paths : null;
        var problem = await Updater.InstallAsync(r, keep);
        if (problem is not null)
            MessageBox.Show(this, "FRIDAY wasn't updated:\n\n" + problem, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
    }

    private void SaveReport()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        var voice = !Speaking.Installed(_install.Paths) ? "not set up" : Speaking.Enabled(_install.Paths) ? "on" : "off";
        var text = SupportReport.Build(Version(), Environment.OSVersion.VersionString,
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
            MessageBox.Show(this, "Couldn't save the report: " + e.Message, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    private IEnumerable<UIElement> General()
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
            SetAdministrator);
        // Features, each with its switch (all on to start). The daemon reads
        // its own (couldnt-check, agent-resume) when it starts, so those restart it.
        yield return Caption("Features");
        var env = EnvFile.Load(_install.Paths.EnvFile);
        foreach (var (name, what) in AppSettings.Features)
            yield return Switch(what, "Switch off if you don't want it.", AppSettings.FeatureOn(env, name),
                on => ChangeEnv(e => AppSettings.WithFeature(e, name, on), restart: name is "couldnt-check" or "agent-resume" or "memory-file" or "screen-secret-check"));
        yield return Row(Pill("Restart FRIDAY", SymbolRegular.ArrowSync24), () => { _do.RestartDaemon(); Said("FRIDAY restarted."); });
        yield return Row(Pill("Open logs", SymbolRegular.DocumentBulletList24), _do.OpenLogs);
    }

    private IEnumerable<UIElement> Privacy()
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
                        "Read a window's text", MessageBoxButton.YesNo, MessageBoxImage.Question, MessageBoxResult.No) != MessageBoxResult.Yes)
                {
                    Show("Privacy");
                    return;
                }
                ChangeEnv(env => AppSettings.WithScreenText(env, on));
            });
        yield return Row(Pill("What left this PC…", SymbolRegular.ArrowUpload24), _do.ShowEgress);
        if (AppSettings.FeatureOn(EnvFile.Load(_install.Paths.EnvFile), "usage-page"))
            yield return Row(Pill("Usage and speed…", SymbolRegular.TopSpeed24), ShowUsage);
        yield return Row(Pill("What FRIDAY remembers about you…", SymbolRegular.Bookmark24), OpenMemoryFile);
        yield return Row(Pill("Bring in notes from another assistant…", SymbolRegular.ArrowImport24), ImportMemory);
        yield return Row(Pill("Forget saved data…", SymbolRegular.Delete24), _do.ForgetSavedData);
    }

    /// <summary>Calls, failures, free-limit hits and speed per provider, from this PC's egress log.</summary>
    private async void ShowUsage()
    {
        var (_, said) = await Ctl.RunAsync(_install, ["usage"], TimeSpan.FromSeconds(15));
        MessageBox.Show(this, said.Trim(), "Usage and speed", MessageBoxButton.OK, MessageBoxImage.Information);
    }

    /// <summary>The memory file in Notepad (made first, with a line saying how).</summary>
    private void OpenMemoryFile()
    {
        var file = _install.Paths.MemoryFile;
        try
        {
            if (!File.Exists(file))
            {
                Directory.CreateDirectory(Path.GetDirectoryName(file)!);
                File.WriteAllText(file, "# What FRIDAY remembers about you. Edit freely: one \"- \" line each.\n");
            }
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo("notepad.exe", $"\"{file}\"") { UseShellExecute = false });
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            MessageBox.Show(this, e.Message, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
    }

    /// <summary>Another assistant's notes (a CLAUDE.md, an exported memory list) into the
    /// memory file: friday-ctl shows what would be added, and adds it only on Yes.</summary>
    private async void ImportMemory()
    {
        var pick = new Microsoft.Win32.OpenFileDialog
        {
            Title = "Notes from another assistant",
            Filter = "Notes (*.md;*.txt)|*.md;*.txt|All files (*.*)|*.*",
        };
        if (pick.ShowDialog(this) != true)
            return;
        var (ok, preview) = await Ctl.RunAsync(_install, ["memory", "import", pick.FileName], TimeSpan.FromSeconds(15));
        if (!ok || !preview.StartsWith("Would add", StringComparison.Ordinal))
        {
            MessageBox.Show(this, preview.Trim(), "FRIDAY", MessageBoxButton.OK, ok ? MessageBoxImage.Information : MessageBoxImage.Warning);
            return;
        }
        var lines = preview.Trim().Split('\n').Where(l => !l.StartsWith("Nothing was added", StringComparison.Ordinal)).ToList();
        var shown = string.Join("\n", lines.Take(21)) + (lines.Count > 21 ? $"\n… and {lines.Count - 21} more" : "");
        if (MessageBox.Show(this, shown + "\n\nAdd them?", "Bring in notes", MessageBoxButton.YesNo, MessageBoxImage.Question, MessageBoxResult.No) != MessageBoxResult.Yes)
            return;
        var (added, said) = await Ctl.RunAsync(_install, ["memory", "import", pick.FileName, "--apply"], TimeSpan.FromSeconds(15));
        MessageBox.Show(this, said.Trim(), "FRIDAY", MessageBoxButton.OK, added ? MessageBoxImage.Information : MessageBoxImage.Warning);
    }

    private IEnumerable<UIElement> Ai()
    {
        yield return Note("FRIDAY thinks with a language model. Free models work with no account; for anything private use your own key or account. Your key is stored only on this PC, in your profile.");
        yield return Row(Pill("Choose a provider and key…", SymbolRegular.Key24, primary: true), _do.ChooseProvider);
        yield return Row(Pill("Connect an account…", SymbolRegular.PersonAccounts24), _do.ConnectAccounts);
    }

    private IEnumerable<UIElement> LocalAiSection()
    {
        yield return Note("Local AI runs on this computer: it works offline, but it uses your computer's memory and processor. Sized to what your machine can handle.");
        yield return BuildLocalAiCard();
    }

    private UIElement BuildLocalAiCard()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        var (isOn, currentModel) = LocalAi.GetCurrentState(env);

        LocalAiInfo? recommendedInfo = null;

        var machineVal = new TextBlock
        {
            Text = "Detecting...",
            FontSize = 13,
            Foreground = Resource("Text"),
            TextWrapping = TextWrapping.Wrap,
        };
        var modelVal = new TextBlock
        {
            Text = "Checking presets...",
            FontSize = 13,
            Foreground = Resource("Text"),
            TextWrapping = TextWrapping.Wrap,
        };
        var stateVal = new TextBlock
        {
            Text = isOn ? $"on: {currentModel}" : "not set up",
            FontSize = 13,
            Foreground = Resource("Text"),
            TextWrapping = TextWrapping.Wrap,
        };
        var progressText = new TextBlock
        {
            FontSize = 12.5,
            LineHeight = 18,
            Foreground = Resource("Subtext"),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 8, 0, 0),
            Visibility = Visibility.Collapsed,
        };
        var progressBar = new System.Windows.Controls.ProgressBar
        {
            Minimum = 0,
            Maximum = 100,
            Height = 6,
            Margin = new Thickness(0, 6, 0, 0),
            Visibility = Visibility.Collapsed,
        };
        AutomationProperties.SetName(progressBar, "Download progress");
        var errorText = new TextBlock
        {
            FontSize = 12.5,
            LineHeight = 18,
            Foreground = Resource("Ember"),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 8, 0, 0),
            Visibility = Visibility.Collapsed,
        };

        // Which engine runs the model: Ollama (its own app and service) or
        // llama.cpp (its release build, run by FRIDAY itself).
        var engine = env.TryGetValue("LLAMACPP_BASE_URL", out var curUrl) && curUrl.TrimEnd('/') == LocalAi.LlamaBaseUrl ? "llamacpp" : "ollama";
        var useOllama = Pill("Ollama", SymbolRegular.Box24);
        var useLlama = Pill("llama.cpp", SymbolRegular.Box24);
        useLlama.Margin = new Thickness(10, 0, 0, 0);
        var engineNote = new TextBlock { FontSize = 12.5, Foreground = Resource("Subtext"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0) };
        void ShowEngine()
        {
            useOllama.Style = (Style)FindResource(engine == "ollama" ? "PrimaryPill" : "Pill");
            useLlama.Style = (Style)FindResource(engine == "llamacpp" ? "PrimaryPill" : "Pill");
            engineNote.Text = engine == "ollama"
                ? "Ollama: installed with winget if it isn't here; it keeps running in the background and other apps can use it too."
                : "llama.cpp: its official build (pinned, checked) runs inside FRIDAY only while local AI is on, and sleeps after five idle minutes.";
        }
        useOllama.Click += (_, _) => { engine = "ollama"; ShowEngine(); };
        useLlama.Click += (_, _) => { engine = "llamacpp"; ShowEngine(); };
        ShowEngine();

        var setupBtn = Pill("Set up local AI", SymbolRegular.ArrowDownload24, primary: true);
        var turnOffBtn = Pill("Turn off", SymbolRegular.Power24);
        turnOffBtn.Visibility = isOn ? Visibility.Visible : Visibility.Collapsed;
        turnOffBtn.Margin = new Thickness(10, 0, 0, 0);

        turnOffBtn.Click += (_, _) =>
        {
            errorText.Visibility = Visibility.Collapsed;
            progressText.Visibility = Visibility.Collapsed;
            ChangeEnv(LocalAi.TurnOffLocalAi);
            _do.LocalAiChanged();
            stateVal.Text = "not set up";
            turnOffBtn.Visibility = Visibility.Collapsed;
        };

        setupBtn.Click += async (_, _) =>
        {
            if (recommendedInfo == null || string.IsNullOrEmpty(recommendedInfo.RecommendedModel))
            {
                errorText.Text = recommendedInfo?.Note ?? "No recommended local model fits this computer.";
                errorText.Visibility = Visibility.Visible;
                return;
            }

            var model = recommendedInfo.RecommendedModel;
            var vision = recommendedInfo.Vision;

            setupBtn.IsEnabled = false;
            turnOffBtn.IsEnabled = false;
            errorText.Visibility = Visibility.Collapsed;
            progressText.Text = "Starting local AI setup...";
            progressText.Visibility = Visibility.Visible;

            var chosen = engine;
            var gpu = recommendedInfo.VramMb is > 0;
            var (success, errorMsg) = await Task.Run<(bool Success, string? Error)>(async () =>
            {
                try
                {
                    if (chosen == "llamacpp")
                    {
                        await Dispatcher.InvokeAsync(() => { progressText.Text = $"Downloading llama.cpp and {model}..."; progressBar.Visibility = Visibility.Visible; });
                        var vis = vision;
                        // A running llama-server would lock the build being replaced.
                        await Dispatcher.InvokeAsync(_do.StopLlama);
                        var (llOk, llOut) = await Ctl.RunAsync(_install, ["local-ai", "install-llamacpp", model, gpu ? "--gpu" : "--cpu"],
                            TimeSpan.FromMinutes(90),
                            line =>
                            {
                                if (LocalAi.ParseProgress(line) is var (p, d, t))
                                    Dispatcher.InvokeAsync(() => { progressBar.Value = p; progressText.Text = $"Downloading: {LocalAi.FormatProgress(p, d, t)}"; });
                                else if (line.StartsWith("done vision="))
                                    vis = line.EndsWith('1');
                            });
                        await Dispatcher.InvokeAsync(() => progressBar.Visibility = Visibility.Collapsed);
                        if (!llOk)
                            return (false, "Could not set up llama.cpp: " + llOut.Trim().Split('\n').LastOrDefault());
                        var (lfOk, lfOut) = await Ctl.RunAsync(_install, ["local-ai", "local-first", model, "auto"], TimeSpan.FromSeconds(15));
                        await Dispatcher.InvokeAsync(() =>
                        {
                            ChangeEnv(e => LocalAi.WithLocalAi(e, model, vis, lfOk && lfOut.Trim() == "1", LocalAi.LlamaBaseUrl));
                            _do.LocalAiChanged();
                            progressText.Text = "Starting llama.cpp...";
                        });
                        return await LlamaReadyAsync() switch
                        {
                            LlamaStart.Ready => (true, null),
                            LlamaStart.Loading => (true, LlamaLoading),
                            _ => (false, LlamaNotReady),
                        };
                    }
                    if (!ProcessRunner.IsOllamaInstalled(out var ollamaExe))
                    {
                        await Dispatcher.InvokeAsync(() => progressText.Text = "Installing Ollama (via winget)...");
                        // winget is an app alias in WindowsApps, which a tray started at sign-in may not have on PATH.
                        var winget = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Microsoft", "WindowsApps", "winget.exe");
                        var (wingetOk, wingetOut) = await ProcessRunner.RunAsync(File.Exists(winget) ? winget : "winget",
                            ["install", "--id", "Ollama.Ollama", "-e", "--disable-interactivity", "--accept-source-agreements", "--accept-package-agreements"],
                            TimeSpan.FromMinutes(10),
                            line => Dispatcher.InvokeAsync(() => progressText.Text = line));
                        // An installer can end non-zero (say, a reboot is wanted) and still have installed it.
                        if (!wingetOk && !ProcessRunner.IsOllamaInstalled(out _))
                        {
                            var last = wingetOut.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).LastOrDefault() ?? "";
                            return (false, "Could not install Ollama with winget" + (last.Length > 0 ? $": {last}" : ".") + " You can install it from ollama.com, then press Set up local AI again.");
                        }

                        if (!ProcessRunner.IsOllamaInstalled(out ollamaExe))
                            ollamaExe = "ollama";
                    }

                    await Dispatcher.InvokeAsync(() => progressText.Text = "Starting Ollama...");
                    if (!await ProcessRunner.EnsureOllamaRunningAsync(ollamaExe))
                        return (false, "Ollama is installed but didn't start. Open Ollama once from the Start menu, then try again.");

                    // Through Ollama's own API, which says how far it is; `ollama
                    // pull` if that can't be had.
                    await Dispatcher.InvokeAsync(() => { progressText.Text = $"Downloading {model}..."; progressBar.Visibility = Visibility.Visible; });
                    var (pullOk, _) = await Ctl.RunAsync(_install, ["local-ai", "pull", model], TimeSpan.FromMinutes(60),
                        line =>
                        {
                            if (LocalAi.ParseProgress(line) is var (p, d, t))
                                Dispatcher.InvokeAsync(() =>
                                {
                                    progressBar.Value = p;
                                    progressText.Text = $"Downloading {model}: {LocalAi.FormatProgress(p, d, t)}";
                                });
                        });
                    if (!pullOk)
                        (pullOk, _) = await ProcessRunner.RunAsync(ollamaExe,
                            ["pull", model],
                            TimeSpan.FromMinutes(30),
                            line => Dispatcher.InvokeAsync(() => progressText.Text = line));
                    await Dispatcher.InvokeAsync(() => progressBar.Visibility = Visibility.Collapsed);
                    if (!pullOk)
                        return (false, $"Could not download {model} with Ollama.");

                    await Dispatcher.InvokeAsync(() => progressText.Text = "Configuring model priority...");
                    var (ctlOk, ctlOut) = await Ctl.RunAsync(_install, ["local-ai", "local-first", model, "auto"], TimeSpan.FromSeconds(15));
                    var localFirst = ctlOk && ctlOut.Trim() == "1";

                    await Dispatcher.InvokeAsync(() =>
                    {
                        ChangeEnv(e => LocalAi.WithLocalAi(e, model, vision, localFirst));
                        _do.LocalAiChanged();
                    });

                    return (true, null);
                }
                catch (Exception ex)
                {
                    return (false, "Local AI setup failed: " + ex.Message);
                }
            });

            setupBtn.IsEnabled = true;
            turnOffBtn.IsEnabled = true;

            if (success)
            {
                // Still loading: on, and says so until it answers.
                progressText.Text = errorMsg ?? "";
                progressText.Visibility = errorMsg is null ? Visibility.Collapsed : Visibility.Visible;
                errorText.Visibility = Visibility.Collapsed;
                stateVal.Text = errorMsg is null ? $"on: {model}" : $"on: {model} (still loading)";
                if (errorMsg is not null)
                    WhenLlamaReady(() => { stateVal.Text = $"on: {model}"; progressText.Visibility = Visibility.Collapsed; });
                turnOffBtn.Visibility = Visibility.Visible;
            }
            else
            {
                progressText.Visibility = Visibility.Collapsed;
                errorText.Text = errorMsg ?? "Local AI setup failed.";
                errorText.Visibility = Visibility.Visible;
            }
        };

        var stack = new StackPanel();
        stack.Children.Add(PairRow("This computer", machineVal));
        stack.Children.Add(new Border { Height = 1, Background = Resource("Hairline"), Opacity = 0.6 });
        stack.Children.Add(PairRow("Recommended model", modelVal));
        stack.Children.Add(new Border { Height = 1, Background = Resource("Hairline"), Opacity = 0.6 });
        stack.Children.Add(PairRow("Current state", stateVal));
        stack.Children.Add(progressText);
        stack.Children.Add(progressBar);
        stack.Children.Add(errorText);

        stack.Children.Add(Caption("Engine"));
        var engineRow = new StackPanel { Orientation = Orientation.Horizontal };
        engineRow.Children.Add(useOllama);
        engineRow.Children.Add(useLlama);
        stack.Children.Add(engineRow);
        stack.Children.Add(engineNote);

        var btnPanel = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 14, 0, 4) };
        btnPanel.Children.Add(setupBtn);
        btnPanel.Children.Add(turnOffBtn);
        stack.Children.Add(btnPanel);

        // Already on this PC: Ollama or llama.cpp with models downloaded
        // before; "Use" answers with one, nothing downloaded.
        var found = new StackPanel { Margin = new Thickness(0, 10, 0, 0) };
        stack.Children.Add(found);
        var busy = false;
        async Task UseAsync(LocalEngine engine, LocalModel m, Button button)
        {
            // One at a time: two would write the same files.
            if (busy)
                return;
            busy = true;
            try
            {
                await UseOneAsync(engine, m, button);
            }
            finally
            {
                busy = false;
            }
        }
        async Task UseOneAsync(LocalEngine engine, LocalModel m, Button button)
        {
            button.IsEnabled = false;
            errorText.Visibility = Visibility.Collapsed;
            if (engine.Engine == "ollama" && !engine.Running
                && !(ProcessRunner.IsOllamaInstalled(out var exe) && await ProcessRunner.EnsureOllamaRunningAsync(exe)))
            {
                errorText.Text = "Ollama didn't start. Open Ollama once from the Start menu, then try again.";
                errorText.Visibility = Visibility.Visible;
                button.IsEnabled = true;
                return;
            }
            // FRIDAY's own llama.cpp file: its llama-server gets that model
            // (also when it is already serving another one).
            var ownLlama = engine.Engine == "llamacpp"
                && File.Exists(Path.Combine(LocalAi.LlamaDir(_install.Paths), "models", m.Name));
            if (ownLlama)
            {
                var (llOk, llOut) = await Ctl.RunAsync(_install, ["local-ai", "install-llamacpp", m.Name], TimeSpan.FromMinutes(5));
                if (!llOk)
                {
                    errorText.Text = "Couldn't use it: " + llOut.Trim().Split('\n').LastOrDefault();
                    errorText.Visibility = Visibility.Visible;
                    button.IsEnabled = true;
                    return;
                }
            }
            var (ok, outText) = await Ctl.RunAsync(_install, ["local-ai", "local-first", m.Name, "auto"], TimeSpan.FromSeconds(15));
            if (!ok)
            {
                errorText.Text = "Couldn't turn it on: " + outText.Trim();
                errorText.Visibility = Visibility.Visible;
                button.IsEnabled = true;
                return;
            }
            ChangeEnv(e => LocalAi.WithLocalAi(e, m.Name, m.Vision, outText.Trim() == "1", engine.BaseUrl));
            _do.LocalAiChanged();
            var start = ownLlama ? await LlamaReadyAsync() : LlamaStart.Ready;
            if (start == LlamaStart.Failed)
            {
                errorText.Text = LlamaNotReady;
                errorText.Visibility = Visibility.Visible;
                button.IsEnabled = true;
                return;
            }
            var on = $"on: {m.Name} ({(engine.Engine == "ollama" ? "Ollama" : "llama.cpp")}, already on this PC";
            stateVal.Text = on + (start == LlamaStart.Loading ? ", still loading)" : ")");
            if (start == LlamaStart.Loading)
            {
                progressText.Text = LlamaLoading;
                progressText.Visibility = Visibility.Visible;
                WhenLlamaReady(() => { stateVal.Text = on + ")"; progressText.Visibility = Visibility.Collapsed; });
            }
            turnOffBtn.Visibility = Visibility.Visible;
            button.IsEnabled = true;
        }
        _ = Task.Run(async () =>
        {
            var (ok, json) = await Ctl.RunAsync(_install, ["local-ai", "engines", "--json"], TimeSpan.FromSeconds(15));
            var engines = ok ? LocalAi.ParseEngines(json).Where(e => e.Installed).ToList() : [];
            await Dispatcher.InvokeAsync(() =>
            {
                if (engines.Count == 0)
                    return;
                found.Children.Add(Caption("Already on this PC"));
                foreach (var engine in engines)
                {
                    found.Children.Add(new TextBlock { Text = engine.Title, FontSize = 12.5, Foreground = Resource("Subtext"), Margin = new Thickness(0, 4, 0, 4) });
                    if (engine.Models.Count == 0)
                        found.Children.Add(new TextBlock { Text = "No models downloaded yet.", FontSize = 12.5, Foreground = Resource("Subtext") });
                    var row = new WrapPanel();
                    foreach (var m in engine.Models)
                    {
                        var use = Pill("Use " + m.Label, SymbolRegular.CheckmarkCircle24);
                        use.Margin = new Thickness(0, 0, 8, 8);
                        // FRIDAY runs its own llama-server for its files; another server must be running.
                        use.IsEnabled = engine.Engine == "ollama" || engine.Running
                            || File.Exists(Path.Combine(LocalAi.LlamaDir(_install.Paths), "models", m.Name));
                        use.Click += async (_, _) => await UseAsync(engine, m, use);
                        row.Children.Add(use);
                    }
                    found.Children.Add(row);
                }
            });
        });

        _ = Task.Run(async () =>
        {
            try
            {
                var (detectOk, detectOut) = await Ctl.RunAsync(_install, ["local-ai", "detect"], TimeSpan.FromSeconds(10));
                var (presetsOk, presetsOut) = await Ctl.RunAsync(_install, ["local-ai", "presets", "--json"], TimeSpan.FromSeconds(10));
                if (detectOk && presetsOk)
                {
                    var info = LocalAi.Parse(detectOut, presetsOut);
                    recommendedInfo = info;
                    await Dispatcher.InvokeAsync(() =>
                    {
                        machineVal.Text = info.MachineSummary;
                        if (!string.IsNullOrEmpty(info.RecommendedModel))
                            modelVal.Text = $"{info.RecommendedModel} ({info.VisionSummary})";
                        else
                            modelVal.Text = info.Note ?? "No local model fits this PC.";
                    });
                }
                else
                {
                    await Dispatcher.InvokeAsync(() =>
                    {
                        machineVal.Text = "Could not detect machine hardware.";
                        modelVal.Text = "Could not load presets.";
                    });
                }
            }
            catch (Exception ex)
            {
                await Dispatcher.InvokeAsync(() =>
                {
                    errorText.Text = "Could not inspect hardware: " + ex.Message;
                    errorText.Visibility = Visibility.Visible;
                });
            }
        });

        return Surface(stack);
    }

    private Grid PairRow(string name, TextBlock valueBlock)
    {
        var grid = new Grid { Tag = PairTag, Margin = new Thickness(0, 9, 0, 9) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(210) });
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        Grid.SetColumn(valueBlock, 1);
        grid.Children.Add(new TextBlock { Text = name, Foreground = Resource("Subtext"), FontSize = 13, TextTrimming = TextTrimming.CharacterEllipsis });
        grid.Children.Add(valueBlock);
        return grid;
    }

    private Button Download(string text, bool primary, string[] setup)
    {
        var button = Pill(text, SymbolRegular.ArrowDownload24, primary);
        button.Click += async (_, _) =>
        {
            button.IsEnabled = false;
            Said("Downloading the voice…");
            var (ok, output) = await Ctl.RunAsync(_install, setup, TimeSpan.FromMinutes(15));
            if (ok)
                Show("Voice");
            else
            {
                button.IsEnabled = true;
                MessageBox.Show(this, "The voice wasn't installed:\n\n" + output.Trim(), "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
            }
        };
        return button;
    }

    private IEnumerable<UIElement> Voice()
    {
        var paths = _install.Paths;
        // Talking to FRIDAY: hold the ask key and speak.
        var key = _do.Hotkeys.TryGetValue(AskKind.Plain, out var k) ? k : "Win+Y";
        yield return Caption("Talk to FRIDAY");
        if (!Listening.Installed(paths))
        {
            yield return Note($"Hold {key} and speak; let go and FRIDAY answers. Your speech becomes text on this PC (whisper.cpp) and only the text is asked, like a typed question. " +
                "It downloads once (about 150 MB, checked against a fixed checksum). A tap still opens the ask box.");
            yield return Download("Set up talking", primary: true, setup: ["listen", "setup"]);
        }
        else
        {
            yield return Note($"Hold {key} and speak; let go and FRIDAY answers. A tap opens the ask box. " +
                "Your speech becomes text on this PC and is never kept. Switch it off under Features (hold-to-talk).");
            yield return Switch(
                "Listen for \"Hey Friday\"",
                "No key at all: say \"Hey Friday, what's the weather?\" (\"Hey Friday, keep listening\" for a conversation; \"Hey Friday, stop\" while it talks). " +
                "The microphone is on all the time while this is on, and uses some processor time whenever someone speaks; what is heard stays on this PC and only your question is asked. " +
                "The tray icon says when it is listening.",
                Listening.WakeOn(paths),
                on => { Listening.SetWake(paths, on); _do.WakeChanged(); });
        }
        yield return Caption("Spoken answers");
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
        var picker = new ComboBox { Width = 240, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 0, 12) };
        AutomationProperties.SetName(picker, "Voice");
        for (var i = 0; i < Speaking.Voices; i++)
            picker.Items.Add($"Voice {i + 1}");
        picker.SelectedIndex = Speaking.Speaker(paths);
        var loading = false;
        picker.SelectionChanged += (_, _) =>
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
            if (names.Count == 0)
                return;
            await picker.Dispatcher.InvokeAsync(() =>
            {
                loading = true;
                var keep = Math.Min(Speaking.Speaker(paths), names.Count - 1);
                picker.Items.Clear();
                foreach (var n in names)
                    picker.Items.Add(n);
                picker.SelectedIndex = keep;
                loading = false;
            });
        });
        yield return Caption("Voice");
        yield return picker;
        yield return Row(Pill("Hear it", SymbolRegular.Play24), () => Ctl.Say(_install, "Hi, I'm FRIDAY. This is how I sound."));
        yield return Row(Pill("Stop", SymbolRegular.Stop24), () => Ctl.StopSaying(_install));
    }

    private IEnumerable<UIElement> Desktop()
    {
        yield return Switch(
            "Let FRIDAY control my desktop when I ask",
            "Say \"focus Notepad\", \"open Brave\", \"set the volume to 30\", \"pause the music\", \"switch to desktop 2\". " +
            "FRIDAY only does this when you ask; closing a window always asks first and shows the exact action. Off: FRIDAY answers questions but does not act.",
            AppSettings.DesktopControlOn(EnvFile.Load(_install.Paths.EnvFile)),
            on => ChangeEnv(env => AppSettings.WithDesktopControl(env, on)));
        // How much it may do without asking: one of three, the current one highlighted.
        yield return Caption("How much FRIDAY may do on its own");
        var mode = AppSettings.AutonomyMode(EnvFile.Load(_install.Paths.EnvFile));
        var modes = new StackPanel { Orientation = Orientation.Horizontal };
        foreach (var (m, label, icon) in new[]
        {
            ("ask-all", "Ask before every action", SymbolRegular.HandRight24),
            ("ask-risky", "Ask only for risky ones", SymbolRegular.Shield24),
            ("act", "Act, then tell me", SymbolRegular.Flash24),
        })
        {
            var pick = Pill(label, icon, primary: m == mode);
            pick.Margin = new Thickness(0, 0, 8, 0);
            pick.Click += (_, _) =>
            {
                ChangeEnv(env => AppSettings.WithAutonomy(env, m));
                Show("Desktop control");
            };
            modes.Children.Add(pick);
        }
        yield return modes;
        yield return Note(mode switch
        {
            "ask-all" => "FRIDAY asks before every action, a window or the volume too.",
            "act" => "FRIDAY does what it would have asked about, then a notification says what it did. The daemon still runs only that exact action.",
            _ => "FRIDAY asks before running a command, writing a file, opening an app or anything that can't be undone, like closing a window.",
        });
        // What FRIDAY did lately, so a window that moved or a program that opened has an explanation.
        var done = ActionLog.ReadNewestFirst(_install.Paths.ActionsFile, 8);
        yield return Caption("What FRIDAY did");
        if (done.Count == 0)
            yield return Pair("Lately", "Nothing yet");
        foreach (var a in done)
            yield return Pair(EgressLog.FormatLocalTime(a.Ts), ActionLog.Describe(a));
    }

    private IEnumerable<UIElement> Agents()
    {
        var env = EnvFile.Load(_install.Paths.EnvFile);
        yield return Switch(
            "Let FRIDAY run coding agents",
            "Run coding agents (such as opencode, agy, or Claude Code) to carry out tasks. Routine steps are approved automatically; risky steps wait for you.",
            AppSettings.AgentsOn(env),
            on => ChangeEnv(e => AppSettings.WithAgents(e, on)));

        yield return Switch(
            "Check an agent's work",
            "When an agent says it's done, FRIDAY runs the project's tests (cargo, npm, go, pytest or make) and says what they show. They run code the agent just wrote.",
            AppSettings.AgentCheckOn(env),
            on => ChangeEnv(e => EnvFile.WithSetting(e, AppSettings.AgentCheck, on ? "1" : null)));

        yield return Switch(
            "Keep an agent's changes apart",
            "In a git folder, each run works on its own branch; you bring its changes in (staged) or drop them from the run below.",
            AppSettings.AgentWorktreeOn(env),
            on => ChangeEnv(e => EnvFile.WithSetting(e, AppSettings.AgentWorktree, on ? "1" : null)));

        yield return AgentHooksSwitch();

        var approved = AgentRuns.HooksApprovedSince(_install.Paths.AgentsDir, DateTimeOffset.UtcNow.AddDays(-1).ToUnixTimeSeconds());
        var steps = approved == 1 ? "1 routine step" : $"{approved} routine steps";
        yield return Note($"FRIDAY approved {steps} in your own sessions in the last day.");

        var runs = AgentRuns.Load(_install.Paths.AgentsDir);
        var recent = runs.Take(10).ToList();
        yield return Caption("Recent runs");
        if (recent.Count == 0)
            yield return Pair("Recent runs", "None yet");
        foreach (var r in recent)
        {
            var status = r.Status == "waiting_for_you" ? "waiting for you" : r.Status;
            var title = $"{r.Agent} · {r.Folder} · {status}";
            var detail = r.Status == "waiting_for_you" && r.Pending is not null
                ? $"{r.Pending.Step} ({r.Pending.Why})"
                : (r.Request ?? "");
            var buttons = new List<Button>();
            if (r.Worktree is not null && r.IsFinished)
            {
                var bring = Pill("Bring changes in", SymbolRegular.ArrowDownload24);
                bring.Click += (_, _) => AgentWorktreeCtl(r.Id, "accept");
                var drop = Pill("Drop changes", SymbolRegular.Delete24);
                drop.Click += (_, _) => AgentWorktreeCtl(r.Id, "discard");
                buttons.Add(bring);
                buttons.Add(drop);
            }
            yield return Card(title, detail, r.IsFinished, [.. buttons]);
        }
    }

    /// <summary>Brings a run's separate changes in ("accept") or drops them ("discard"), then says what happened.</summary>
    private async void AgentWorktreeCtl(string id, string verb)
    {
        try
        {
            var (ok, output) = await Ctl.RunAsync(_install, ["agents", verb, id], TimeSpan.FromSeconds(60));
            if (ok)
                Said(output.Trim());
            else
                MessageBox.Show(this, output.Trim(), "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
        catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or InvalidOperationException)
        {
            MessageBox.Show(this, e.Message, "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
        Show("Agents", animate: false);
    }

    private bool _updatingHooks;

    private UIElement AgentHooksSwitch()
    {
        const string title = "Answer routine approvals in my own agent sessions";
        const string detail = "FRIDAY answers routine permissions in the Claude Code, agy and opencode sessions you start yourself; risky actions still wait for you.";
        var toggle = new ToggleSwitch
        {
            IsEnabled = false,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(16, 0, 0, 0),
        };
        AutomationProperties.SetName(toggle, title);

        toggle.Checked += (_, _) =>
        {
            if (_updatingHooks) return;
            SetAgentHooks(toggle, true);
        };
        toggle.Unchecked += (_, _) =>
        {
            if (_updatingHooks) return;
            SetAgentHooks(toggle, false);
        };

        _ = Task.Run(async () =>
        {
            var (ok, output) = await Ctl.RunAsync(_install, ["agent-hooks", "status"], TimeSpan.FromSeconds(5));
            var isOn = ok && output.Contains("FRIDAY answers routine approvals");
            await Dispatcher.InvokeAsync(() =>
            {
                _updatingHooks = true;
                toggle.IsChecked = isOn;
                _updatingHooks = false;
                toggle.IsEnabled = true;
            });
        });

        var text = new StackPanel();
        text.Children.Add(new TextBlock { Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold, Foreground = Resource("Text"), TextWrapping = TextWrapping.Wrap });
        text.Children.Add(new TextBlock { Text = detail, FontSize = 12.5, LineHeight = 18, Foreground = Resource("Subtext"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 4, 0, 0) });
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(toggle, 1);
        grid.Children.Add(text);
        grid.Children.Add(toggle);
        return Surface(grid);
    }

    private void SetAgentHooks(ToggleSwitch toggle, bool on)
    {
        toggle.IsEnabled = false;
        _ = Task.Run(async () =>
        {
            var (ok, output) = await Ctl.RunAsync(_install, ["agent-hooks", on ? "on" : "off"], TimeSpan.FromSeconds(5));
            await Dispatcher.InvokeAsync(() =>
            {
                toggle.IsEnabled = true;
                if (ok)
                {
                    Said(on ? "Approvals in your own agent sessions turned on." : "Approvals in your own agent sessions turned off.");
                }
                else
                {
                    _updatingHooks = true;
                    toggle.IsChecked = !on;
                    _updatingHooks = false;
                    MessageBox.Show(this, "Couldn't update FRIDAY's agent hooks:\n\n" + output.Trim(), "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
                }
            });
        });
    }

    private IEnumerable<UIElement> Tasks()
    {
        yield return Note("Things FRIDAY does again and again. Make one by asking, like \"every morning at 8 tell me the weather\" " +
            "or \"when the battery is under 20% tell me\". Each one runs like your own ask: the answer opens, and anything risky still asks first. " +
            "They run while FRIDAY is running.");
        var path = _install.Paths.StandingFile;
        yield return Switch(
            "Quiet at night",
            "From 22:00 to 07:00 tasks that repeat wait until morning; reminders and battery alerts still come. At most 50 task runs a day either way.",
            StandingTasks.Quiet(path) is not null,
            on => { StandingTasks.SetQuiet(path, on); Show("Tasks"); });
        var tasks = StandingTasks.Load(path);
        if (tasks.Count == 0)
            yield return Pair("Standing tasks", "None yet");
        foreach (var t in tasks)
        {
            var pause = Pill(t.Paused ? "Resume" : "Pause", t.Paused ? SymbolRegular.Play24 : SymbolRegular.Pause24);
            pause.Click += (_, _) =>
            {
                StandingTasks.SetPaused(path, t.Id, !t.Paused);
                Show("Tasks");
                Said(t.Paused ? "Resumed." : "Paused: it won't run until you resume it.");
            };
            var delete = Pill("Delete", SymbolRegular.Delete24);
            delete.Click += (_, _) =>
            {
                StandingTasks.Delete(path, t.Id);
                Show("Tasks");
                Said("Deleted.");
            };
            yield return Card(t.Goal, t.When + (t.Paused ? " · paused" : ""), t.Paused, pause, delete);
        }
    }

    private IEnumerable<UIElement> Shortcuts()
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

    private IEnumerable<UIElement> About()
    {
        yield return Pair("FRIDAY", Version());
        yield return Pair("Installed in", _install.Dir.TrimEnd('\\'));
        yield return Pair("Your data", _install.Paths.EnvFile.Replace("\\friday.env", ""));
        yield return Row(Pill("Open the data folder", SymbolRegular.FolderOpen24), _do.OpenDataFolder);
    }

    // ---- building blocks ----

    private static string Version()
    {
        var v = typeof(SettingsWindow).Assembly.GetName().Version;
        return $"{v?.Major}.{v?.Minor}.{v?.Build}";
    }

    private Brush Resource(string key) => (Brush)FindResource(key);

    private Button NavButton(string name, SymbolRegular icon)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal };
        content.Children.Add(new SymbolIcon { Symbol = icon, FontSize = 16, Margin = new Thickness(0, 0, 12, 0), VerticalAlignment = VerticalAlignment.Center });
        content.Children.Add(new TextBlock { Text = name, FontSize = 13.5, VerticalAlignment = VerticalAlignment.Center });
        var b = new Button
        {
            Content = content,
            Height = 36,
            Margin = new Thickness(0, 0, 0, 2),
            Padding = new Thickness(14, 0, 10, 0),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Left,
            Background = Brushes.Transparent,
            BorderThickness = new Thickness(0),
            Foreground = Resource("Subtext"),
            Cursor = System.Windows.Input.Cursors.Hand,
            Style = (Style)FindResource("NavItem"),
        };
        AutomationProperties.SetName(b, name);
        b.Click += (_, _) => Show(name);
        return b;
    }

    private Button Pill(string text, SymbolRegular icon, bool primary = false)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal };
        content.Children.Add(new SymbolIcon { Symbol = icon, FontSize = 14, Margin = new Thickness(0, 0, 7, 0), VerticalAlignment = VerticalAlignment.Center });
        content.Children.Add(new TextBlock { Text = text, VerticalAlignment = VerticalAlignment.Center });
        var b = new Button { Content = content, Style = (Style)FindResource(primary ? "PrimaryPill" : "Pill") };
        AutomationProperties.SetName(b, text);
        return b;
    }

    /// <summary>A setting: what it is and what it does, and its switch.</summary>
    private UIElement Switch(string title, string detail, bool on, Action<bool> set, bool enabled = true)
    {
        var toggle = new ToggleSwitch { IsChecked = on, IsEnabled = enabled, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(16, 0, 0, 0) };
        AutomationProperties.SetName(toggle, title);
        toggle.Checked += (_, _) => set(true);
        toggle.Unchecked += (_, _) => set(false);
        var text = new StackPanel();
        text.Children.Add(new TextBlock { Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold, Foreground = Resource(enabled ? "Text" : "Subtext"), TextWrapping = TextWrapping.Wrap });
        text.Children.Add(new TextBlock { Text = detail, FontSize = 12.5, LineHeight = 18, Foreground = Resource("Subtext"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 4, 0, 0) });
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(toggle, 1);
        grid.Children.Add(text);
        grid.Children.Add(toggle);
        return Surface(grid);
    }

    /// <summary>A card with a title, a line under it, and buttons on the right.</summary>
    private UIElement Card(string title, string detail, bool dim, params Button[] buttons)
    {
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(new TextBlock { Text = title, FontSize = 14, FontWeight = FontWeights.SemiBold, Foreground = Resource(dim ? "Subtext" : "Text"), TextWrapping = TextWrapping.Wrap });
        text.Children.Add(new TextBlock { Text = detail, FontSize = 12.5, Foreground = Resource("Subtext"), Margin = new Thickness(0, 3, 0, 0) });
        var row = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        foreach (var b in buttons)
            row.Children.Add(b);
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(row, 1);
        grid.Children.Add(text);
        grid.Children.Add(row);
        return Surface(grid);
    }

    private Border Surface(UIElement child) => new()
    {
        Background = Resource("Well"),
        BorderBrush = Resource("Hairline"),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(10),
        Padding = new Thickness(16, 13, 16, 13),
        Margin = new Thickness(0, 0, 0, 10),
        Child = child,
    };

    private UIElement Note(string text) => new TextBlock
    {
        Text = text,
        FontSize = 12.5,
        LineHeight = 18,
        Foreground = Resource("Subtext"),
        TextWrapping = TextWrapping.Wrap,
        Margin = new Thickness(2, 4, 0, 14),
    };

    private UIElement Caption(string text) => new TextBlock
    {
        Text = text.ToUpperInvariant(),
        FontSize = 11,
        FontWeight = FontWeights.SemiBold,
        Foreground = Resource("Faint"),
        Margin = new Thickness(2, 10, 0, 8),
    };

    private static Button Row(Button button, Action click)
    {
        button.Click += (_, _) => click();
        return button;
    }

    /// <summary>A label and its value; neighbouring pairs share one card, divided by hairlines.</summary>
    private UIElement Pair(string name, string value)
    {
        var grid = new Grid { Tag = PairTag, Margin = new Thickness(0, 9, 0, 9) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(210) });
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        var v = new TextBlock { Text = value, Foreground = Resource("Text"), FontSize = 13, TextWrapping = TextWrapping.Wrap };
        Grid.SetColumn(v, 1);
        grid.Children.Add(new TextBlock { Text = name, Foreground = Resource("Subtext"), FontSize = 13, TextTrimming = TextTrimming.CharacterEllipsis });
        grid.Children.Add(v);
        return grid;
    }

    private static readonly object PairTag = new();

    /// <summary>Neighbouring pairs into one card; neighbouring buttons into one row.</summary>
    private IEnumerable<UIElement> Group(IEnumerable<UIElement> items)
    {
        StackPanel? pairs = null;
        WrapPanel? buttons = null;
        foreach (var e in items)
        {
            if (e is Grid { Tag: var tag } && ReferenceEquals(tag, PairTag))
            {
                buttons = null;
                if (pairs is null)
                {
                    pairs = new StackPanel();
                    var card = Surface(pairs);
                    card.Padding = new Thickness(16, 4, 16, 4);
                    yield return card;
                }
                else
                    pairs.Children.Add(new Border { Height = 1, Background = Resource("Hairline"), Opacity = 0.6 });
                pairs.Children.Add(e);
                continue;
            }
            pairs = null;
            if (e is Button b)
            {
                if (buttons is null)
                {
                    buttons = new WrapPanel { Margin = new Thickness(-6, 0, 0, 6) };
                    yield return buttons;
                }
                b.Margin = new Thickness(6, 0, 0, 8); // room between wrapped rows
                buttons.Children.Add(b);
                continue;
            }
            buttons = null;
            yield return e;
        }
    }

    /// <summary>"Saved. FRIDAY restarted.": a toast that rises in, waits, and fades.</summary>
    private void Said(string text)
    {
        ToastText.Text = text;
        var move = new TranslateTransform(0, 10);
        Toast.RenderTransform = move;
        move.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(0, TimeSpan.FromMilliseconds(420)) { EasingFunction = new Spring { Damping = 0.7 } });
        var fade = new DoubleAnimationUsingKeyFrames();
        fade.KeyFrames.Add(new EasingDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(200))));
        fade.KeyFrames.Add(new LinearDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(2600))));
        fade.KeyFrames.Add(new EasingDoubleKeyFrame(0, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(3000))));
        Toast.BeginAnimation(OpacityProperty, fade);
    }

    private const string LlamaNotReady =
        "llama.cpp didn't start answering. Another program may be using port 8080, or the model doesn't fit this PC's memory; FRIDAY's logs say more.";

    private const string LlamaLoading =
        "llama.cpp is still loading the model (slow on this PC); FRIDAY uses it as soon as it's ready and the free models until then.";

    private enum LlamaStart { Ready, Loading, Failed }

    private static readonly string LlamaHealth =
        LocalAi.LlamaBaseUrl[..LocalAi.LlamaBaseUrl.LastIndexOf("/v1", StringComparison.Ordinal)] + "/health";

    private static async Task<bool> LlamaHealthyAsync(System.Net.Http.HttpClient http)
    {
        try
        {
            return (await http.GetAsync(LlamaHealth)).IsSuccessStatusCode;
        }
        catch (Exception e) when (e is System.Net.Http.HttpRequestException or TaskCanceledException)
        {
            return false;
        }
    }

    /// <summary>Runs <paramref name="ready"/> on the UI thread once llama-server answers (up to 30 minutes).</summary>
    private void WhenLlamaReady(Action ready) => _ = Task.Run(async () =>
    {
        using var http = new System.Net.Http.HttpClient { Timeout = TimeSpan.FromSeconds(3) };
        for (var deadline = DateTime.UtcNow.AddMinutes(30); DateTime.UtcNow < deadline; await Task.Delay(5000))
            if (await LlamaHealthyAsync(http))
            {
                await Dispatcher.InvokeAsync(ready);
                return;
            }
    });

    /// <summary>Whether FRIDAY's llama-server answers its health check within 3 minutes (a big model loads
    /// slowly); FRIDAY's own still running after that, and for over 90 s (not crash-looping), is loading.</summary>
    private async Task<LlamaStart> LlamaReadyAsync()
    {
        using var http = new System.Net.Http.HttpClient { Timeout = TimeSpan.FromSeconds(3) };
        for (var deadline = DateTime.UtcNow.AddMinutes(3); DateTime.UtcNow < deadline; await Task.Delay(1000))
            if (await LlamaHealthyAsync(http))
                return LlamaStart.Ready;
        return _do.LlamaUptime() > TimeSpan.FromSeconds(90) ? LlamaStart.Loading : LlamaStart.Failed;
    }

    /// <summary>Rewrites friday.env with <paramref name="change"/>; the daemon
    /// restarts to read it unless the setting is only the tray's
    /// (<paramref name="restart"/> false).</summary>
    private void ChangeEnv(Func<string, string> change, bool restart = true)
    {
        var path = _install.Paths.EnvFile;
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, change(File.Exists(path) ? File.ReadAllText(path) : ""));
        File.Move(tmp, path, overwrite: true);
        if (!restart)
        {
            Said("Saved.");
            return;
        }
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
                "Start with administrator rights", MessageBoxButton.OKCancel, MessageBoxImage.Information) != MessageBoxResult.OK)
        {
            Show("General");
            return;
        }
        if (!Elevation.Apply(_install, on))
        {
            MessageBox.Show(this, "Windows didn't allow the change (the prompt was declined, or this account can't approve it). Nothing was changed.",
                "FRIDAY", MessageBoxButton.OK, MessageBoxImage.Warning);
            Show("General");
            return;
        }
        // The task now starts FRIDAY at sign-in (or the Run entry does again).
        _install.StartsAtLogin = !on;
        Show("General");
        if (MessageBox.Show(this, on ? "FRIDAY will start as administrator from now on. Restart it now?" : "FRIDAY will start normally from now on. Restart it now?",
                "FRIDAY", MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes)
            _do.RestartIntoAdministrator();
    }
}
