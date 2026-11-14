using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Choose the AI provider and paste its key (the Windows `friday setup-ai`).
/// Writes friday.env; the key is never shown again or logged.
/// </summary>
internal sealed class ProviderForm : Form
{
    private readonly ComboBox _provider;
    private readonly Label _freeNotice;
    private readonly Label _connectedSummary;
    private readonly TextBox _key;
    private readonly CheckBox _keepOthers;
    private readonly Label _message;
    private readonly string _envPath;
    private readonly Action? _restartDaemon;

    public ProviderForm(string envPath, Action? restartDaemon = null)
    {
        _envPath = envPath;
        _restartDaemon = restartDaemon;
        Text = "FRIDAY · AI provider";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(460, 370);
        Padding = new Padding(18);

        var layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, BackColor = Theme.Bg };
        layout.Controls.Add(new Label { Text = "FRIDAY thinks with a cloud language model. Pick a provider:", AutoSize = true, ForeColor = Theme.Subtext });

        _provider = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList, Width = 420, BackColor = Theme.Surface, ForeColor = Theme.Text, FlatStyle = FlatStyle.Flat };
        foreach (var p in Providers.All)
            _provider.Items.Add(p.Name);
        _provider.SelectedIndex = 0;
        layout.Controls.Add(_provider);

        _freeNotice = new Label
        {
            Text = "Free models may keep what you send them, including screenshots and selected text, and may use it for training. For anything private, use your own key or account.",
            AutoSize = true,
            MaximumSize = new Size(420, 0),
            ForeColor = Theme.Subtext,
            Margin = new Padding(0, 6, 0, 4),
        };
        layout.Controls.Add(_freeNotice);

        var connectRow = new FlowLayoutPanel
        {
            AutoSize = true,
            FlowDirection = FlowDirection.LeftToRight,
            WrapContents = false,
            BackColor = Theme.Bg,
            Margin = new Padding(0, 4, 0, 4),
        };
        var connectBtn = Theme.Button("Connect an account…");
        connectBtn.Click += (_, _) =>
        {
            using var dlg = new AccountsForm(_envPath, _restartDaemon);
            dlg.ShowDialog(this);
            UpdateConnectedSummary();
            UpdateKeepOthers();
        };
        connectRow.Controls.Add(connectBtn);

        _connectedSummary = new Label
        {
            AutoSize = true,
            ForeColor = Theme.Subtext,
            Font = Theme.Small,
            Margin = new Padding(8, 6, 0, 0),
        };
        connectRow.Controls.Add(_connectedSummary);
        layout.Controls.Add(connectRow);

        layout.Controls.Add(new Label { Text = "API key (optional for free models):", AutoSize = true, ForeColor = Theme.Subtext, Margin = new Padding(0, 8, 0, 0) });
        _key = new TextBox { UseSystemPasswordChar = true, Width = 420, BackColor = Theme.Surface, ForeColor = Theme.Text, BorderStyle = BorderStyle.FixedSingle };
        layout.Controls.Add(_key);

        _keepOthers = new CheckBox
        {
            Text = "Keep my other keys too (FRIDAY falls back to them)",
            AutoSize = true,
            ForeColor = Theme.Subtext,
            Margin = new Padding(0, 8, 0, 0),
        };
        layout.Controls.Add(_keepOthers);

        _message = new Label { AutoSize = true, ForeColor = Theme.Error, Margin = new Padding(0, 6, 0, 0) };
        layout.Controls.Add(_message);
        var save = Theme.Button("Save", primary: true);
        save.Click += (_, _) => Save();
        layout.Controls.Add(save);
        Controls.Add(layout);
        AcceptButton = save;

        _provider.SelectedIndexChanged += (_, _) => UpdateKeepOthers();
        UpdateConnectedSummary();
        UpdateKeepOthers();
    }

    private void UpdateKeepOthers()
    {
        var cur = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        _keepOthers.Visible = EnvFile.HasOtherKeys(cur, Providers.All[_provider.SelectedIndex]);
        _freeNotice.Visible = (_provider.SelectedIndex == 0);
    }

    private void UpdateConnectedSummary()
    {
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var ids = AgentClis.GetIds(existing);
        if (ids.Count == 0)
        {
            _connectedSummary.Text = "";
        }
        else
        {
            var labels = ids.Select(id => AgentClis.Find(id) is { } c ? c.BinaryName : id);
            _connectedSummary.Text = "Connected: " + string.Join(", ", labels);
        }
    }

    private void Save()
    {
        var provider = Providers.All[_provider.SelectedIndex];
        var key = _key.Text.Trim();
        if (!(provider.KeyOptional && key.Length == 0) && !Providers.LooksLikeKey(key))
        {
            _message.Text = "That doesn't look like an API key (too short, or contains spaces/symbols).";
            return;
        }
        Directory.CreateDirectory(Path.GetDirectoryName(_envPath)!);
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var tmp = _envPath + ".tmp";
        File.WriteAllText(tmp, EnvFile.WithProvider(existing, provider, key, _keepOthers.Visible && _keepOthers.Checked));
        File.Move(tmp, _envPath, overwrite: true);
        DialogResult = DialogResult.OK;
        Close();
    }
}
