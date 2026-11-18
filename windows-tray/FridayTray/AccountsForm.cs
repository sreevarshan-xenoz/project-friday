using System.Diagnostics;
using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Small dialog listing the four account tools (Claude Code, Codex, Antigravity, Cline)
/// to connect or disconnect them via their own CLIs.
/// </summary>
internal sealed class AccountsForm : Form
{
    private readonly string _envPath;
    private readonly Action? _restartDaemon;
    private readonly ListBox _accountsList;
    private readonly Label _statusLabel;
    private readonly TextBox _infoBox;
    private readonly Button _connectBtn;
    private readonly Button _disconnectBtn;
    private readonly Button _closeBtn;
    private CancellationTokenSource? _checkCts;

    public AccountsForm(string envPath, Action? restartDaemon = null)
    {
        _envPath = envPath;
        _restartDaemon = restartDaemon;

        Text = "FRIDAY · Connect an account";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        StartPosition = FormStartPosition.CenterParent;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(500, 400);
        Padding = new Padding(18);

        var layout = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            ColumnCount = 1,
            BackColor = Theme.Bg,
        };

        var desc = new Label
        {
            Text = "Use an AI account you already have. FRIDAY asks through its own app with your sign-in; it never sees your password or token.",
            AutoSize = true,
            MaximumSize = new Size(464, 0),
            ForeColor = Theme.Subtext,
            Margin = new Padding(0, 0, 0, 10),
        };
        layout.Controls.Add(desc);

        _accountsList = new ListBox
        {
            Width = 464,
            Height = 90,
            BackColor = Theme.Surface,
            ForeColor = Theme.Text,
            BorderStyle = BorderStyle.FixedSingle,
            Font = Theme.Body,
            Margin = new Padding(0, 0, 0, 8),
        };
        _accountsList.SelectedIndexChanged += (_, _) => UpdateSelectionState();
        layout.Controls.Add(_accountsList);

        _statusLabel = new Label
        {
            Text = "Status: Not connected",
            AutoSize = true,
            Font = Theme.Small,
            ForeColor = Theme.Subtext,
            Margin = new Padding(0, 0, 0, 4),
        };
        layout.Controls.Add(_statusLabel);

        _infoBox = new TextBox
        {
            Multiline = true,
            ReadOnly = true,
            Width = 464,
            Height = 96,
            BackColor = Theme.Surface,
            ForeColor = Theme.Text,
            BorderStyle = BorderStyle.FixedSingle,
            Font = Theme.Small,
            Margin = new Padding(0, 0, 0, 10),
        };
        layout.Controls.Add(_infoBox);

        var buttons = new FlowLayoutPanel
        {
            AutoSize = true,
            FlowDirection = FlowDirection.LeftToRight,
            WrapContents = false,
            BackColor = Theme.Bg,
            Margin = new Padding(0),
        };

        _connectBtn = Theme.Button("Connect", primary: true);
        _connectBtn.Click += async (_, _) => await HandleConnectAsync();
        buttons.Controls.Add(_connectBtn);

        _disconnectBtn = Theme.Button("Disconnect");
        _disconnectBtn.Margin = new Padding(8, 0, 0, 0);
        _disconnectBtn.Click += (_, _) => HandleDisconnect();
        buttons.Controls.Add(_disconnectBtn);

        _closeBtn = Theme.Button("Close");
        _closeBtn.Margin = new Padding(8, 0, 0, 0);
        _closeBtn.Click += (_, _) => Close();
        buttons.Controls.Add(_closeBtn);

        layout.Controls.Add(buttons);
        Controls.Add(layout);
        CancelButton = _closeBtn;

        RefreshList();
    }

    private AgentCli? SelectedCli =>
        _accountsList.SelectedIndex >= 0 && _accountsList.SelectedIndex < AgentClis.All.Count
            ? AgentClis.All[_accountsList.SelectedIndex]
            : null;

    private void RefreshList()
    {
        var sel = _accountsList.SelectedIndex;
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var connected = AgentClis.GetIds(existing).ToHashSet(StringComparer.OrdinalIgnoreCase);

        _accountsList.BeginUpdate();
        _accountsList.Items.Clear();
        foreach (var cli in AgentClis.All)
        {
            var isConn = connected.Contains(cli.Id);
            _accountsList.Items.Add($"{cli.Label}{(isConn ? "  ✓ Connected" : "")}");
        }
        if (sel >= 0 && sel < _accountsList.Items.Count)
            _accountsList.SelectedIndex = sel;
        else if (_accountsList.Items.Count > 0)
            _accountsList.SelectedIndex = 0;
        _accountsList.EndUpdate();
    }

    private void UpdateSelectionState()
    {
        var cli = SelectedCli;
        if (cli == null)
        {
            _statusLabel.Text = "";
            _infoBox.Text = "";
            _connectBtn.Enabled = false;
            _disconnectBtn.Visible = false;
            return;
        }

        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var isConnected = AgentClis.HasId(existing, cli.Id);

        if (isConnected)
        {
            _statusLabel.Text = "Status: Connected";
            _statusLabel.ForeColor = Theme.Signal;
            _infoBox.Text = $"Connected {cli.Label}.\r\nFRIDAY uses it when the free models can't answer (it starts the app for each answer and counts against your plan).";
            _connectBtn.Text = "Check again";
            _disconnectBtn.Visible = true;
            _disconnectBtn.Enabled = true;
        }
        else
        {
            _statusLabel.Text = "Status: Not connected";
            _statusLabel.ForeColor = Theme.Subtext;
            _disconnectBtn.Visible = false;

            var binary = AgentClis.FindCli(cli);
            if (binary == null)
            {
                _infoBox.Text = $"{cli.Label} needs its app, '{cli.BinaryName}', which isn't installed.\r\nInstall hint:\r\n  {cli.InstallHint}";
                _connectBtn.Text = "Check again";
            }
            else
            {
                _infoBox.Text = $"Click 'Connect' to verify sign-in for {cli.Label}.";
                if (_connectBtn.Text != "Check again")
                    _connectBtn.Text = "Connect";
            }
        }
        _connectBtn.Enabled = true;
    }

    private async Task HandleConnectAsync()
    {
        var cli = SelectedCli;
        if (cli == null)
            return;

        var binary = AgentClis.FindCli(cli);
        if (binary == null)
        {
            _statusLabel.Text = "Status: App not installed";
            _statusLabel.ForeColor = Theme.Error;
            _infoBox.Text = $"{cli.Label} needs its app, '{cli.BinaryName}', which isn't installed.\r\nInstall it with:\r\n\r\n  {cli.InstallHint}\r\n\r\nAfter installing, click 'Check again'.";
            _connectBtn.Text = "Check again";
            return;
        }

        _statusLabel.Text = "Status: Checking sign-in (up to 120s)…";
        _statusLabel.ForeColor = Theme.Warn;
        _connectBtn.Enabled = false;
        _disconnectBtn.Enabled = false;

        _checkCts?.Dispose();
        _checkCts = new CancellationTokenSource();
        var cancel = _checkCts.Token;

        try
        {
            var answered = await AgentClis.RunCheckAsync(cli, binary, timeout: TimeSpan.FromSeconds(120), cancel: cancel);
            // The dialog was closed while the check ran: nothing to show, nothing to restart.
            if (cancel.IsCancellationRequested || IsDisposed)
                return;
            if (!answered)
            {
                try
                {
                    var comspec = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
                    Process.Start(new ProcessStartInfo(comspec, $"/k {cli.LoginCommand}")
                    {
                        UseShellExecute = true,
                    });
                }
                catch (Exception ex)
                {
                    _infoBox.Text = $"Could not launch sign-in console: {ex.Message}";
                }

                _statusLabel.Text = "Status: Sign-in required";
                _statusLabel.ForeColor = Theme.Warn;
                _infoBox.Text = $"Not signed in yet.\r\n\r\nA console window has opened to sign in ({cli.LoginCommand}).\r\nComplete sign-in in that window, then return here and press 'Check again'.";
                _connectBtn.Text = "Check again";
            }
            else
            {
                SaveEnv(cli.Id, add: true);
                _restartDaemon?.Invoke();

                _statusLabel.Text = "Status: Connected";
                _statusLabel.ForeColor = Theme.Signal;
                _infoBox.Text = $"Connected {cli.Label}.\r\nFRIDAY uses it when the free models can't answer (it starts the app for each answer and counts against your plan).";
                _connectBtn.Text = "Check again";
                RefreshList();
            }
        }
        finally
        {
            if (!IsDisposed)
            {
                _connectBtn.Enabled = true;
                UpdateSelectionState();
            }
        }
    }

    private void HandleDisconnect()
    {
        var cli = SelectedCli;
        if (cli == null)
            return;

        SaveEnv(cli.Id, add: false);
        _restartDaemon?.Invoke();

        _statusLabel.Text = "Status: Not connected";
        _statusLabel.ForeColor = Theme.Subtext;
        _infoBox.Text = $"FRIDAY no longer uses {cli.Label}.\r\n(You stay signed in to it; sign out there if you want.)";
        _connectBtn.Text = "Connect";
        RefreshList();
        UpdateSelectionState();
    }

    private void SaveEnv(string id, bool add)
    {
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var updated = add ? AgentClis.AddId(existing, id) : AgentClis.RemoveId(existing, id);
        Directory.CreateDirectory(Path.GetDirectoryName(_envPath)!);
        var tmp = _envPath + ".tmp";
        File.WriteAllText(tmp, updated);
        File.Move(tmp, _envPath, overwrite: true);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _checkCts?.Cancel();
            _checkCts?.Dispose();
        }
        base.Dispose(disposing);
    }
}
