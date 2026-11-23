using System.Windows;
using System.Windows.Controls;
using FridayTray.Core;
using Wpf.Ui.Controls;
using CheckBox = System.Windows.Controls.CheckBox;
using ComboBox = System.Windows.Controls.ComboBox;
using Orientation = System.Windows.Controls.Orientation;
using PasswordBox = System.Windows.Controls.PasswordBox;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// Choose the AI provider and paste its key (the Windows `friday setup-ai`).
/// Writes friday.env; the key is never shown again or logged.
/// </summary>
internal sealed class ProviderWindow : CardWindow
{
    private readonly ComboBox _provider = new() { Margin = new Thickness(0, 0, 0, 12) };
    private readonly Border _freeNotice;
    private readonly TextBlock _connectedSummary = new() { FontSize = 12.5, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(10, 0, 0, 0) };
    private readonly PasswordBox _key = new() { Margin = new Thickness(0, 6, 0, 10) };
    private readonly CheckBox _keepOthers = new() { Content = "Keep my other keys too (FRIDAY falls back to them)", Margin = new Thickness(0, 0, 0, 6) };
    private readonly TextBlock _message = new() { FontSize = 12.5, TextWrapping = TextWrapping.Wrap };
    private readonly string _envPath;

    public ProviderWindow(string envPath, Action? restartDaemon = null)
        : base("FRIDAY · AI provider", "Your AI", 500)
    {
        _envPath = envPath;
        Body.Children.Add(Note("FRIDAY thinks with a cloud language model. Pick a provider:"));
        foreach (var p in Providers.All)
            _provider.Items.Add(p.Name);
        _provider.SelectedIndex = 0;
        Body.Children.Add(_provider);

        var warn = new StackPanel { Orientation = Orientation.Horizontal };
        warn.Children.Add(new SymbolIcon { Symbol = SymbolRegular.ShieldKeyhole24, FontSize = 15, Foreground = R("Warn"), Margin = new Thickness(0, 1, 10, 0), VerticalAlignment = VerticalAlignment.Top });
        warn.Children.Add(new TextBlock
        {
            Text = "Free models may keep what you send them, including screenshots and selected text, and may use it for training. For anything private, use your own key or account.",
            FontSize = 12.5,
            LineHeight = 18,
            Foreground = R("Subtext"),
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 420,
        });
        _freeNotice = Well(warn);
        Body.Children.Add(_freeNotice);

        var connect = Pill("Connect an account…", SymbolRegular.PersonAccounts24);
        connect.Click += (_, _) =>
        {
            new AccountsWindow(_envPath, restartDaemon) { Owner = this }.ShowDialog();
            UpdateConnectedSummary();
            UpdateKeepOthers();
        };
        var connectRow = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(-6, 0, 0, 14) };
        connectRow.Children.Add(connect);
        _connectedSummary.Foreground = R("Signal");
        connectRow.Children.Add(_connectedSummary);
        Body.Children.Add(connectRow);

        Body.Children.Add(new TextBlock { Text = "API key (optional for free models)", FontSize = 12.5, Foreground = R("Subtext") });
        Body.Children.Add(_key);
        _keepOthers.Foreground = R("Subtext");
        Body.Children.Add(_keepOthers);
        _message.Foreground = R("Error");
        Body.Children.Add(_message);

        var cancel = Pill("Cancel");
        cancel.IsCancel = true;
        cancel.Click += (_, _) => Close();
        var save = Pill("Save", SymbolRegular.Checkmark24, primary: true);
        save.IsDefault = true;
        save.Click += (_, _) => Save();
        Buttons.Children.Add(cancel);
        Buttons.Children.Add(save);

        _provider.SelectionChanged += (_, _) => UpdateKeepOthers();
        UpdateConnectedSummary();
        UpdateKeepOthers();
    }

    private void UpdateKeepOthers()
    {
        var cur = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        _keepOthers.Visibility = EnvFile.HasOtherKeys(cur, Providers.All[_provider.SelectedIndex]) ? Visibility.Visible : Visibility.Collapsed;
        _freeNotice.Visibility = _provider.SelectedIndex == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void UpdateConnectedSummary()
    {
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var ids = AgentClis.GetIds(existing);
        _connectedSummary.Text = ids.Count == 0
            ? ""
            : "Connected: " + string.Join(", ", ids.Select(id => AgentClis.Find(id) is { } c ? c.BinaryName : id));
    }

    private void Save()
    {
        var provider = Providers.All[_provider.SelectedIndex];
        var key = _key.Password.Trim();
        if (!(provider.KeyOptional && key.Length == 0) && !Providers.LooksLikeKey(key))
        {
            _message.Text = "That doesn't look like an API key (too short, or contains spaces/symbols).";
            return;
        }
        Directory.CreateDirectory(Path.GetDirectoryName(_envPath)!);
        var existing = File.Exists(_envPath) ? File.ReadAllText(_envPath) : "";
        var tmp = _envPath + ".tmp";
        var text = EnvFile.WithProvider(existing, provider, key, _keepOthers.Visibility == Visibility.Visible && _keepOthers.IsChecked == true);
        // The key goes to Credential Manager; keys of providers just replaced are removed from it.
        var kept = EnvFile.Parse(text).Keys;
        foreach (var gone in EnvFile.Parse(existing).Keys.Where(k => Keyring.IsSecret(k) && !kept.Contains(k)))
            CredentialStore.Delete(gone);
        if (key.Length > 0)
            text = Keyring.Store(text, provider.KeyVar, key, CredentialStore.Write, CredentialStore.Delete);
        File.WriteAllText(tmp, text);
        File.Move(tmp, _envPath, overwrite: true);
        DialogResult = true;
    }
}
