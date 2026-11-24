using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Color = System.Windows.Media.Color;
using KeyEventArgs = System.Windows.Input.KeyEventArgs;
using TextBlock = System.Windows.Controls.TextBlock;

namespace FridayTray.Fluent;

/// <summary>
/// The ask box: a frosted bar a quarter of the way down the screen the mouse
/// is on. Enter asks, Escape or clicking away closes, Up/Down walks the recent
/// questions. A selection or screen ask says what goes with it in a chip; with
/// a selection the quick actions are listed under the field (click one, or
/// walk them with Up/Down; what you type instead is sent as typed).
/// </summary>
public partial class AskWindow : Window
{
    private static readonly Color Clear = Color.FromArgb(0, 255, 255, 255);
    private static readonly Color Hover = Color.FromArgb(0x14, 255, 255, 255);
    private static readonly Color Chosen = Color.FromArgb(0x26, 0xFF, 0x8F, 0x4C);

    private readonly IReadOnlyList<string> _history;
    private readonly List<Border> _rows = [];
    private int _index = -1;
    private bool _wasActive;
    private bool _done;
    private bool _modal;

    public string Question => Input.Text.Trim();

    public AskWindow(AskKind kind, string? contextPreview, string? sensitiveFrom, IReadOnlyList<string> history)
    {
        InitializeComponent();
        _history = history;

        switch (kind)
        {
            case AskKind.Selection:
                if (sensitiveFrom is null)
                    ShowContext(SymbolRegular.TextQuote24, $"“{contextPreview}…”", Resource("Subtext"));
                else
                    ShowContext(SymbolRegular.Warning24, $"“{contextPreview}…” · from {sensitiveFrom}, so someone else's words: Enter sends them to your AI provider", Resource("Warn"));
                Placeholder.Text = history.Count > 0 ? "Ask about it, or pick one below…" : "Ask about it…";
                Keys.Text = "↑↓ choose   ·   ↵ ask   ·   esc close";
                foreach (var action in history)
                    AddRow(action);
                Quick.Visibility = _rows.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
                break;
            case AskKind.Screen:
                ShowContext(sensitiveFrom is null ? SymbolRegular.Camera24 : SymbolRegular.Warning24, sensitiveFrom is null
                    ? "A screenshot of your screen goes to your AI provider, this once"
                    : $"This is {sensitiveFrom}, so someone else's words: Enter sends a screenshot of it to your AI provider", Resource("Warn"));
                Placeholder.Text = "Ask about your screen…";
                Keys.Text = "↵ ask   ·   esc close";
                break;
            default:
                if (contextPreview is not null)
                    ShowContext(SymbolRegular.ArrowReply24, contextPreview, Resource("Subtext"));
                Keys.Text = history.Count > 0 ? "↑↓ recent questions   ·   ↵ ask   ·   esc close" : "↵ ask   ·   esc close";
                break;
        }

        Input.TextChanged += (_, _) => OnTyped();
        FolderChip.MouseLeftButtonUp += (_, _) => PickFolder();
        ThisChip.MouseLeftButtonUp += (_, _) =>
        {
            AboutWindow = true;
            Finish(true);
        };
        PreviewKeyDown += OnKey;
        Activated += (_, _) => _wasActive = true;
        Closing += (_, _) => _done = true; // closing deactivates it too
        // Clicking away closes it, but only once it has had focus: Windows may
        // refuse a background app the foreground, and an ask box that vanishes
        // the moment it opens would be worse than one that stays.
        Deactivated += (_, _) =>
        {
            if (_wasActive)
                Finish(false);
        };
        Loaded += (_, _) =>
        {
            Place();
            Motion.RiseIn(Frame, dy: -10, ms: 480, fromScale: 0.97);
            for (var i = 0; i < _rows.Count; i++)
                Motion.RiseIn(_rows[i], dy: 6, delayMs: 60 + Math.Min(i, 8) * 28, ms: 380);
            Breathe();
            Activate();
            Input.Focus();
            Keyboard.Focus(Input);
        };
    }

    /// <summary>Agents' folders, for the folder chip (null: agents are off).</summary>
    public AgentFolders? Folders { get; init; }

    /// <summary>Whether a vague "this" may mean the window in front (one is there, not private or mail).</summary>
    public bool OfferWindow { get; init; }

    /// <summary>Asks that fit the app in front, shown first (↑ walks them before recent questions).</summary>
    public IReadOnlyList<string> Suggested
    {
        init
        {
            foreach (var s in value)
                AddRow(s);
            Quick.Visibility = _rows.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        }
    }

    /// <summary>The question's "this" is the window in front: ask about the screen.</summary>
    public bool AboutWindow { get; private set; }

    private void OnTyped()
    {
        var chip = Folders?.Chip(Input.Text);
        FolderChip.Visibility = chip is null ? Visibility.Collapsed : Visibility.Visible;
        FolderText.Text = chip ?? "";
        ThisChip.Visibility = OfferWindow && VagueThis.Matches(Input.Text) ? Visibility.Visible : Visibility.Collapsed;
        var empty = Input.Text.Length == 0;
        Placeholder.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        EnterKey.BeginAnimation(OpacityProperty, new DoubleAnimation(empty ? 0 : 1, TimeSpan.FromMilliseconds(160)) { EasingFunction = Motion.Out });
    }

    private void OnKey(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Escape:
                Finish(false);
                break;
            case Key.Enter when Question.Length > 0:
                AboutWindow = ThisChip.Visibility == Visibility.Visible && (Keyboard.Modifiers & ModifierKeys.Control) != 0;
                Finish(true);
                break;
            case Key.Up when _history.Count > 0:
                Walk(Math.Min(_history.Count - 1, _index + 1));
                break;
            case Key.Down when _index > 0:
                Walk(_index - 1);
                break;
            default:
                return;
        }
        e.Handled = true;
    }

    /// <summary>The folders agents have worked in; picking one adds "in &lt;folder&gt;" to the request.</summary>
    private void PickFolder()
    {
        if (Folders is not { Known.Count: > 0 } folders)
            return;
        var menu = new ContextMenu { PlacementTarget = FolderChip };
        foreach (var folder in folders.Known)
        {
            var shown = folders.Tilde(folder);
            var item = new System.Windows.Controls.MenuItem { Header = shown };
            item.Click += (_, _) =>
            {
                Input.Text = Input.Text.TrimEnd() + " in " + shown;
                Input.CaretIndex = Input.Text.Length;
                Input.Focus();
            };
            menu.Items.Add(item);
        }
        menu.IsOpen = true;
    }

    /// <summary>Puts the <paramref name="i"/>th recent question or quick action in the field.</summary>
    private void Walk(int i)
    {
        _index = i;
        Input.Text = _history[i];
        Input.CaretIndex = Input.Text.Length;
        for (var r = 0; r < _rows.Count; r++)
            Tint(_rows[r], r == i ? Chosen : Clear);
    }

    /// <summary>Shows it as the dialog the tray waits on (true: ask <see cref="Question"/>).</summary>
    public new bool? ShowDialog()
    {
        _modal = true;
        return base.ShowDialog();
    }

    private void Finish(bool ok)
    {
        if (_done)
            return;
        _done = true;
        if (_modal)
            DialogResult = ok;
        else
            Close(); // shown on its own (the screenshots)
    }

    // ---- look ----

    private void ShowContext(SymbolRegular icon, string text, Brush color)
    {
        ContextIcon.Symbol = icon;
        ContextIcon.Foreground = color;
        ContextText.Text = text;
        ContextText.Foreground = color;
        Context.Visibility = Visibility.Visible;
    }

    private void AddRow(string action)
    {
        var index = _rows.Count;
        var row = new Border
        {
            CornerRadius = new CornerRadius(6),
            Padding = new Thickness(10, 6, 10, 6),
            Margin = new Thickness(0, 0, 0, 2),
            Background = new SolidColorBrush(Clear),
            Cursor = System.Windows.Input.Cursors.Hand,
            Child = new TextBlock { Text = action, FontSize = 13, Foreground = Resource("Text"), TextTrimming = TextTrimming.CharacterEllipsis },
        };
        row.MouseEnter += (_, _) => { if (index != _index) Tint(row, Hover); };
        row.MouseLeave += (_, _) => { if (index != _index) Tint(row, Clear); };
        // A click picks one and asks it straight away.
        row.MouseLeftButtonUp += (_, _) =>
        {
            Input.Text = action;
            Finish(true);
        };
        _rows.Add(row);
        Quick.Items.Add(row);
    }

    private static void Tint(Border row, Color to) =>
        ((SolidColorBrush)row.Background).BeginAnimation(SolidColorBrush.ColorProperty,
            new ColorAnimation(to, TimeSpan.FromMilliseconds(140)) { EasingFunction = Motion.Out });

    /// <summary>The orb's slow breath: FRIDAY is listening.</summary>
    private void Breathe()
    {
        var breath = new DoubleAnimation(1.0, 1.55, TimeSpan.FromMilliseconds(1800))
        {
            AutoReverse = true,
            RepeatBehavior = RepeatBehavior.Forever,
            EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut },
        };
        HaloScale.BeginAnimation(ScaleTransform.ScaleXProperty, breath);
        HaloScale.BeginAnimation(ScaleTransform.ScaleYProperty, breath);
    }

    /// <summary>Centred, a quarter of the way down the screen the mouse is on.</summary>
    private void Place()
    {
        var area = System.Windows.Forms.Screen.FromPoint(System.Windows.Forms.Cursor.Position).WorkingArea;
        var fromDevice = PresentationSource.FromVisual(this)?.CompositionTarget?.TransformFromDevice ?? Matrix.Identity;
        var left = area.Left * fromDevice.M11;
        var width = area.Width * fromDevice.M11;
        Left = left + (width - ActualWidth) / 2;
        Top = (area.Top + area.Height / 4.0) * fromDevice.M22;
    }

    private Brush Resource(string key) => (Brush)FindResource(key);
}
