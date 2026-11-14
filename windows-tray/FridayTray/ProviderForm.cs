using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Choose the AI provider and paste its key (the Windows `friday setup-ai`).
/// Writes friday.env; the key is never shown again or logged.
/// </summary>
internal sealed class ProviderForm : Form
{
    private readonly ComboBox _provider;
    private readonly TextBox _key;
    private readonly CheckBox _keepOthers;
    private readonly Label _message;
    private readonly string _envPath;

    public ProviderForm(string envPath)
    {
        _envPath = envPath;
        Text = "FRIDAY · AI provider";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(460, 262);
        Padding = new Padding(18);

        var layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, BackColor = Theme.Bg };
        layout.Controls.Add(new Label { Text = "FRIDAY thinks with a cloud language model. Pick a provider:", AutoSize = true, ForeColor = Theme.Subtext });
        _provider = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList, Width = 420, BackColor = Theme.Surface, ForeColor = Theme.Text, FlatStyle = FlatStyle.Flat };
        foreach (var p in Providers.All)
            _provider.Items.Add(p.Name);
        _provider.SelectedIndex = 0;
        layout.Controls.Add(_provider);
        layout.Controls.Add(new Label { Text = "API key (optional for OpenCode Zen):", AutoSize = true, ForeColor = Theme.Subtext, Margin = new Padding(0, 10, 0, 0) });
        _key = new TextBox { UseSystemPasswordChar = true, Width = 420, BackColor = Theme.Surface, ForeColor = Theme.Text, BorderStyle = BorderStyle.FixedSingle };
        layout.Controls.Add(_key);
        // Shown only when other providers' keys are saved: keep them (FRIDAY
        // falls back to them) or use only this provider.
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        _keepOthers = new CheckBox
        {
            Text = "Keep my other keys too (FRIDAY falls back to them)",
            AutoSize = true,
            ForeColor = Theme.Subtext,
            Margin = new Padding(0, 8, 0, 0),
        };
        void UpdateKeepOthers() =>
            _keepOthers.Visible = EnvFile.HasOtherKeys(existing, Providers.All[_provider.SelectedIndex]);
        _provider.SelectedIndexChanged += (_, _) => UpdateKeepOthers();
        UpdateKeepOthers();
        layout.Controls.Add(_keepOthers);
        _message = new Label { AutoSize = true, ForeColor = Theme.Error, Margin = new Padding(0, 6, 0, 0) };
        layout.Controls.Add(_message);
        var save = Theme.Button("Save", primary: true);
        save.Click += (_, _) => Save();
        layout.Controls.Add(save);
        Controls.Add(layout);
        AcceptButton = save;
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
