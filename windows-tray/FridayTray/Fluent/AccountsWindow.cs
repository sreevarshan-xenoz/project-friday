using System.Diagnostics;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Button = System.Windows.Controls.Button;
using Color = System.Windows.Media.Color;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// The account tools FRIDAY can ask through (Claude Code, Codex, Antigravity,
/// Cline): pick one to connect or disconnect it through its own app.
/// </summary>
internal sealed class AccountsWindow : CardWindow
{
    private static readonly Color Picked = Color.FromArgb(0x26, 0xFF, 0x8F, 0x4C);

    private readonly string _envPath;
    private readonly Action? _restartDaemon;
    private readonly StackPanel _list = new();
    private readonly TextBlock _status = new() { FontSize = 12.5, FontWeight = FontWeights.SemiBold, Margin = new Thickness(2, 2, 0, 6) };
    private readonly TextBlock _info = new() { FontSize = 12.5, LineHeight = 18, TextWrapping = TextWrapping.Wrap };
    private readonly Button _connect;
    private readonly Button _disconnect;
    private int _selected;
    private CancellationTokenSource? _checkCts;

    public AccountsWindow(string envPath, Action? restartDaemon = null)
        : base("FRIDAY · Connect an account", "Connect an account", 500)
    {
        _envPath = envPath;
        _restartDaemon = restartDaemon;
        Body.Children.Add(Note("Use an AI account you already have. FRIDAY asks through its own app with your sign-in; it never sees your password or token."));
        Body.Children.Add(_list);
        Body.Children.Add(_status);
        _info.Foreground = R("Text");
        Body.Children.Add(Well(_info, new Thickness(14, 12, 14, 12)));

        _connect = Pill("Connect", SymbolRegular.PlugConnected24, primary: true);
        _connect.Click += async (_, _) => await ConnectAsync();
        _disconnect = Pill("Disconnect", SymbolRegular.PlugDisconnected24);
        _disconnect.Click += (_, _) => Disconnect();
        var close = Pill("Close");
        close.IsCancel = true;
        close.Click += (_, _) => Close();
        Buttons.Children.Add(close);
        Buttons.Children.Add(_disconnect);
        Buttons.Children.Add(_connect);

        Closed += (_, _) => _checkCts?.Cancel();
        RefreshList();
        UpdateSelection();
    }

    private AgentCli Selected => AgentClis.All[_selected];

    private string Env => File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";

    /// <summary>One card per account; the picked one is tinted ember.</summary>
    private void RefreshList()
    {
        var connected = AgentClis.GetIds(Env).ToHashSet(StringComparer.OrdinalIgnoreCase);
        _list.Children.Clear();
        for (var i = 0; i < AgentClis.All.Count; i++)
        {
            var cli = AgentClis.All[i];
            var index = i;
            var row = new Grid();
            row.ColumnDefinitions.Add(new ColumnDefinition());
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new TextBlock { Text = cli.Label, FontSize = 13.5, FontWeight = FontWeights.SemiBold, Foreground = R("Text"), VerticalAlignment = VerticalAlignment.Center });
            if (connected.Contains(cli.Id))
            {
                var tag = new Border
                {
                    CornerRadius = new CornerRadius(9),
                    Padding = new Thickness(9, 2, 9, 3),
                    Background = new SolidColorBrush(Color.FromArgb(0x26, 0x5F, 0xD3, 0xBC)),
                    Child = new TextBlock { Text = "Connected", FontSize = 11.5, Foreground = R("Signal") },
                };
                Grid.SetColumn(tag, 1);
                row.Children.Add(tag);
            }
            var card = new Border
            {
                CornerRadius = new CornerRadius(10),
                BorderThickness = new Thickness(1),
                BorderBrush = i == _selected ? R("Ember") : R("Hairline"),
                Background = i == _selected ? new SolidColorBrush(Picked) : R("Well"),
                Padding = new Thickness(14, 10, 14, 10),
                Margin = new Thickness(0, 0, 0, 6),
                Cursor = System.Windows.Input.Cursors.Hand,
                Child = row,
            };
            AutomationProperties.SetName(card, cli.Label + (connected.Contains(cli.Id) ? ", connected" : ""));
            card.MouseLeftButtonUp += (_, e) =>
            {
                e.Handled = true;
                _selected = index;
                RefreshList();
                UpdateSelection();
            };
            _list.Children.Add(card);
        }
    }

    private void Status(string text, string color)
    {
        _status.Text = text;
        _status.Foreground = R(color);
    }

    private void SetConnectText(string text) =>
        ((TextBlock)((StackPanel)_connect.Content).Children[1]).Text = text;

    private void UpdateSelection()
    {
        var cli = Selected;
        if (AgentClis.HasId(Env, cli.Id))
        {
            Status("Connected", "Signal");
            _info.Text = $"Connected {cli.Label}. FRIDAY uses it when the free models can't answer (it starts the app for each answer and counts against your plan).";
            SetConnectText("Check again");
            _disconnect.Visibility = Visibility.Visible;
        }
        else
        {
            Status("Not connected", "Subtext");
            _disconnect.Visibility = Visibility.Collapsed;
            if (AgentClis.FindCli(cli) is null)
            {
                _info.Text = $"{cli.Label} needs its app, '{cli.BinaryName}', which isn't installed. Install it with:\n\n{cli.InstallHint}";
                SetConnectText("Check again");
            }
            else
            {
                _info.Text = $"Press Connect to check that you're signed in to {cli.Label}.";
                SetConnectText("Connect");
            }
        }
        _connect.IsEnabled = true;
    }

    private async Task ConnectAsync()
    {
        var cli = Selected;
        if (AgentClis.FindCli(cli) is not { } binary)
        {
            Status("App not installed", "Error");
            _info.Text = $"{cli.Label} needs its app, '{cli.BinaryName}', which isn't installed. Install it with:\n\n{cli.InstallHint}\n\nThen press Check again.";
            SetConnectText("Check again");
            return;
        }
        Status("Checking sign-in (up to 2 minutes)…", "Warn");
        _connect.IsEnabled = false;
        _disconnect.IsEnabled = false;
        _checkCts?.Dispose();
        _checkCts = new CancellationTokenSource();
        var cancel = _checkCts.Token;
        try
        {
            var answered = await AgentClis.RunCheckAsync(cli, binary, timeout: TimeSpan.FromSeconds(120), cancel: cancel);
            // Closed while the check ran: nothing to show, nothing to restart.
            if (cancel.IsCancellationRequested || IsClosed)
                return;
            if (!answered)
            {
                try
                {
                    var comspec = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
                    Process.Start(new ProcessStartInfo(comspec, $"/k {cli.LoginCommand}") { UseShellExecute = true });
                    Status("Sign-in required", "Warn");
                    _info.Text = $"Not signed in yet. A console window has opened to sign in ({cli.LoginCommand}). Finish signing in there, then press Check again.";
                }
                catch (Exception ex)
                {
                    Status("Sign-in required", "Warn");
                    _info.Text = $"Couldn't open the sign-in console: {ex.Message}";
                }
                SetConnectText("Check again");
                _connect.IsEnabled = true;
                _disconnect.IsEnabled = true;
                return;
            }
            SaveEnv(cli.Id, add: true);
            _restartDaemon?.Invoke();
            RefreshList();
        }
        finally
        {
            _disconnect.IsEnabled = true;
        }
        if (!IsClosed)
            UpdateSelection();
    }

    private void Disconnect()
    {
        var cli = Selected;
        SaveEnv(cli.Id, add: false);
        _restartDaemon?.Invoke();
        RefreshList();
        UpdateSelection();
        _info.Text = $"FRIDAY no longer uses {cli.Label}. (You stay signed in to it; sign out there if you want.)";
    }

    private void SaveEnv(string id, bool add)
    {
        var updated = add ? AgentClis.AddId(Env, id) : AgentClis.RemoveId(Env, id);
        Directory.CreateDirectory(Path.GetDirectoryName(_envPath)!);
        var tmp = _envPath + ".tmp";
        File.WriteAllText(tmp, updated);
        File.Move(tmp, _envPath, overwrite: true);
    }
}
