using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Button = System.Windows.Controls.Button;
using Color = System.Windows.Media.Color;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using KeyEventArgs = System.Windows.Input.KeyEventArgs;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// First run: what FRIDAY is, the keys, where it lives, and the two choices
/// worth making now (which AI, and whether to look at Settings). Everything
/// else stays off.
/// </summary>
public partial class WelcomeWindow : Window
{
    /// <summary>Closed; the tray makes a new one when asked again.</summary>
    public bool IsClosed { get; private set; }

    public WelcomeWindow(IReadOnlyDictionary<AskKind, string> keys, Action chooseProvider, Action openSettings, Action tryIt, Action? connectBrowser = null)
    {
        InitializeComponent();
        string Key(AskKind k, string fallback) => keys.TryGetValue(k, out var l) ? l : fallback;
        Keys.Children.Add(KeyRow(Key(AskKind.Plain, "Tray icon"), "Ask anything"));
        Keys.Children.Add(KeyRow(Key(AskKind.Selection, "Tray menu"), "Ask about the text you selected"));
        Keys.Children.Add(KeyRow(Key(AskKind.Screen, "Tray menu"), "Ask about your screen (a screenshot goes to your AI provider, this once)"));

        var provider = Pill("Choose my AI…", "Pill");
        provider.Click += (_, _) => chooseProvider();
        // The browsers already on this PC: offered right away.
        var browser = Pill("Connect my browser…", "Pill");
        browser.Click += (_, _) => (connectBrowser ?? openSettings)();
        var settings = Pill("Settings…", "Pill");
        settings.Click += (_, _) => openSettings();
        var done = Pill("Done", "Pill");
        done.IsCancel = true;
        done.Click += (_, _) => Close();
        var tryNow = Pill("Try it now", "PrimaryPill");
        tryNow.IsDefault = true;
        tryNow.Click += (_, _) => { Close(); tryIt(); };
        foreach (var b in new[] { provider, browser, settings, done, tryNow })
            Buttons.Children.Add(b);

        CloseButton.Click += (_, _) => Close();
        Closed += (_, _) => IsClosed = true;
        Loaded += (_, _) => Enter();
        // No title bar: the card itself moves the window.
        Card.MouseLeftButtonDown += (_, e) => { if (e.ButtonState == System.Windows.Input.MouseButtonState.Pressed) DragMove(); };
    }

    /// <summary>The card rises in; the orb springs up and starts to glow; then the words, then each key.</summary>
    private void Enter()
    {
        Motion.RiseIn(Frame, dy: 14, ms: 620, fromScale: 0.96);
        var pop = new DoubleAnimation(0.6, 1, TimeSpan.FromMilliseconds(900)) { EasingFunction = new Spring { Damping = 0.42, Frequency = 1.4 } };
        OrbScale.BeginAnimation(ScaleTransform.ScaleXProperty, pop);
        OrbScale.BeginAnimation(ScaleTransform.ScaleYProperty, pop);
        var breath = new DoubleAnimation(1, 1.35, TimeSpan.FromMilliseconds(2200))
        {
            AutoReverse = true,
            RepeatBehavior = RepeatBehavior.Forever,
            EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut },
        };
        HaloScale.BeginAnimation(ScaleTransform.ScaleXProperty, breath);
        HaloScale.BeginAnimation(ScaleTransform.ScaleYProperty, breath);

        var delay = 120;
        foreach (UIElement e in new UIElement[] { Hello, Intro })
        {
            Motion.RiseIn(e, dy: 10, delayMs: delay, ms: 560);
            delay += 70;
        }
        foreach (UIElement e in Keys.Children)
        {
            Motion.RiseIn(e, dy: 10, delayMs: delay, ms: 520);
            delay += 60;
        }
        foreach (UIElement e in new UIElement[] { Tray, Note, Buttons })
        {
            Motion.RiseIn(e, dy: 8, delayMs: delay, ms: 500);
            delay += 50;
        }
    }

    /// <summary>A card: the key, drawn as keycaps, and what it does.</summary>
    private Border KeyRow(string key, string what)
    {
        var caps = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        var parts = key.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
        for (var i = 0; i < parts.Length; i++)
        {
            if (i > 0)
                caps.Children.Add(new TextBlock { Text = "+", Margin = new Thickness(5, 0, 5, 0), Foreground = Resource("Faint"), VerticalAlignment = VerticalAlignment.Center });
            caps.Children.Add(Keycap(parts[i]));
        }
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(168) });
        grid.ColumnDefinitions.Add(new ColumnDefinition());
        var text = new TextBlock { Text = what, Style = (Style)FindResource("Body"), FontSize = 13.5, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(text, 1);
        grid.Children.Add(caps);
        grid.Children.Add(text);
        var card = new Border
        {
            Background = Resource("Well"),
            BorderBrush = Resource("Hairline"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(14, 10, 14, 10),
            Margin = new Thickness(0, 0, 0, 8),
            Child = grid,
        };
        AutomationProperties.SetName(card, $"{key}: {what}");
        return card;
    }

    /// <summary>A key as it looks on a keyboard: lit from above, with a darker lip.</summary>
    private static Border Keycap(string label) => new()
    {
        MinWidth = 30,
        Padding = new Thickness(8, 3, 8, 4),
        CornerRadius = new CornerRadius(6),
        BorderThickness = new Thickness(1, 1, 1, 2.5),
        BorderBrush = new SolidColorBrush(Color.FromArgb(0xCC, 0x0A, 0x0B, 0x0E)),
        Background = new LinearGradientBrush(Color.FromRgb(0x34, 0x38, 0x42), Color.FromRgb(0x24, 0x27, 0x2F), 90),
        Child = new TextBlock
        {
            Text = label,
            FontSize = 12,
            FontWeight = FontWeights.SemiBold,
            Foreground = new SolidColorBrush(Color.FromRgb(0xF2, 0xEE, 0xE8)),
            HorizontalAlignment = HorizontalAlignment.Center,
        },
    };

    private Button Pill(string text, string style) => new() { Content = text, Style = (Style)FindResource(style) };

    private Brush Resource(string key) => (Brush)FindResource(key);
}
