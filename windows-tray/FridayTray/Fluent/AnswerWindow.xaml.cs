using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Shapes;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Block = FridayTray.Core.Block;
using Brush = System.Windows.Media.Brush;
using Button = System.Windows.Controls.Button;
using Clipboard = System.Windows.Forms.Clipboard;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using KeyEventArgs = System.Windows.Input.KeyEventArgs;
using Orientation = System.Windows.Controls.Orientation;
using Span = FridayTray.Core.Span;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// An answer, above the tray: the question, the answer (it streams in, then
/// settles into formatted text), where it came from, and what to do next.
/// Opens as soon as the question is asked (<see cref="ShowWaiting"/>), fills
/// in while the answer streams (<see cref="ShowPartial"/>), then shows the
/// outcome (<see cref="ShowOutcome"/>): an answer, a step that needs the
/// person's OK, or a failure (with Start FRIDAY when FRIDAY isn't running).
/// </summary>
public partial class AnswerWindow : Window
{
    public event Action? AskAgain;
    public event Action? TryAnother;
    public event Action? FollowUp;
    public event Action? StartFriday;
    public event Action<string>? ConfirmRun;

    /// <summary>Whether "Try another model" applies (a text ask, not a screen one).</summary>
    public bool CanAskAnother { get; set; }

    /// <summary>Closed (by the person or the tray); a new ask needs a new window.</summary>
    public bool IsClosed { get; private set; }

    private readonly string _question;
    private TextBlock? _stream;
    private bool _closing;

    public AnswerWindow(string question, string waiting = "Thinking…")
    {
        InitializeComponent();
        _question = question;
        Question.Text = question;
        CloseButton.Click += (_, _) => CloseSoftly();
        KeyDown += OnKey;
        Closed += (_, _) => IsClosed = true;
        SizeChanged += (_, _) => Place();
        Loaded += (_, _) =>
        {
            Fit(animate: false);
            Place();
            Motion.RiseIn(Frame, dy: 16, ms: 560, fromScale: 0.97);
        };
        ShowWaiting(waiting);
    }

    /// <summary>A window showing a finished answer.</summary>
    public AnswerWindow(string question, AskOutcome outcome) : this(question) => ShowOutcome(outcome);

    /// <summary>Back to waiting (a confirmed step running, or asking again).</summary>
    public void ShowWaiting(string waiting = "Thinking…")
    {
        SetOrb(Resource("Ember"), breathing: true);
        Footer.Text = "";
        Footer.Visibility = Visibility.Collapsed;
        Buttons.Children.Clear();
        _stream = null;
        Blocks.Children.Clear();
        Scroll.Visibility = Visibility.Collapsed;
        Thinking.Visibility = Visibility.Visible;
        ThinkingText.Text = waiting;
        Wave(true);
        Refit();
    }

    /// <summary>The answer so far, while it streams; the newest words stay in view.</summary>
    public void ShowPartial(string text)
    {
        if (_stream is null)
        {
            Wave(false);
            Thinking.Visibility = Visibility.Collapsed;
            Blocks.Children.Clear();
            _stream = new TextBlock { Style = (Style)FindResource("Body") };
            AutomationProperties.SetAutomationId(_stream, "Answer");
            Blocks.Children.Add(_stream);
            Scroll.Visibility = Visibility.Visible;
            Motion.FadeIn(Scroll);
        }
        _stream.Inlines.Clear();
        _stream.Inlines.Add(new Run(text));
        _stream.Inlines.Add(new InlineUIContainer(Caret()) { BaselineAlignment = BaselineAlignment.Center });
        AutomationProperties.SetName(_stream, text); // the words, without the caret
        Scroll.ScrollToEnd();
        Refit();
    }

    /// <summary>The final answer, the step waiting for an OK, or the failure, with what to do next.</summary>
    public void ShowOutcome(AskOutcome outcome)
    {
        var streamed = _stream is not null;
        _stream = null;
        Wave(false);
        Thinking.Visibility = Visibility.Collapsed;
        Scroll.Visibility = Visibility.Visible;
        Blocks.Children.Clear();
        Buttons.Children.Clear();
        Footer.Text = "";
        Footer.Visibility = Visibility.Collapsed;

        if (outcome.Confirmation is { } conf)
        {
            SetOrb(Resource("Warn"), breathing: false);
            Blocks.Children.Add(new TextBlock
            {
                Style = (Style)FindResource("Body"),
                Foreground = Resource("Subtext"),
                Text = "FRIDAY needs your OK before it does this:",
                Margin = new Thickness(0, 0, 0, 8),
            });
            Blocks.Children.Add(CodeWell(outcome.Answer, Resource("Text")));
            var cancel = Pill("Cancel", SymbolRegular.Dismiss24);
            cancel.IsCancel = true;
            cancel.IsDefault = true; // Enter doesn't run anything by accident
            cancel.Click += (_, _) => CloseSoftly();
            var run = Pill("Run", SymbolRegular.Play24, primary: true);
            run.Click += (_, _) => ConfirmRun?.Invoke(conf.Action);
            AddButtons(run, cancel);
            Settle(streamed);
            cancel.Focus();
            Refit();
            return;
        }

        if (outcome.Ok)
        {
            SetOrb(Resource("Ember"), breathing: false);
            foreach (var block in MarkdownBlocks.Parse(outcome.Answer))
                Blocks.Children.Add(Render(block));
        }
        else
        {
            SetOrb(Resource("Error"), breathing: false);
            var failed = new TextBlock { Style = (Style)FindResource("Body"), Text = outcome.Answer };
            AutomationProperties.SetAutomationId(failed, "Answer");
            Blocks.Children.Add(failed);
        }
        Footer.Text = Providers.FormatFooter(outcome.Ok ? outcome.ServedBy : null, outcome.Masked, outcome.Ok ? outcome.Used : null) ?? "";
        Footer.Visibility = Footer.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;

        var buttons = new List<Button>();
        if (outcome.DaemonDown)
        {
            var start = Pill("Start FRIDAY", SymbolRegular.Play24, primary: true);
            start.IsDefault = true;
            start.Click += (_, _) => { StartFriday?.Invoke(); CloseSoftly(); };
            buttons.Add(start);
        }
        else
        {
            if (outcome.Ok)
            {
                var follow = Pill("Follow up", SymbolRegular.Chat24);
                follow.Click += (_, _) => { FollowUp?.Invoke(); CloseSoftly(); };
                buttons.Add(follow);
                if (CanAskAnother)
                {
                    var another = Pill("Another model", SymbolRegular.Sparkle24);
                    another.ToolTip = "Ask a different model, told to take more care";
                    another.Click += (_, _) => { TryAnother?.Invoke(); CloseSoftly(); };
                    buttons.Add(another);
                }
            }
            var again = Pill("Ask again", SymbolRegular.ArrowClockwise24);
            again.Click += (_, _) => { AskAgain?.Invoke(); CloseSoftly(); };
            buttons.Add(again);
            var copy = Pill("Copy", SymbolRegular.Copy24, primary: outcome.Ok);
            copy.IsDefault = outcome.Ok;
            copy.Click += (_, _) => Copy(copy, outcome.Answer);
            buttons.Add(copy);
        }
        AddButtons(buttons.ToArray());
        Settle(streamed);
        Refit();
    }

    // ---- motion ----

    /// <summary>A streamed answer just sharpens into its final form; a new one rises in, block by block.</summary>
    private void Settle(bool streamed)
    {
        if (streamed)
            Motion.FadeIn(Blocks, 260);
        else
            for (var i = 0; i < Blocks.Children.Count; i++)
                Motion.RiseIn(Blocks.Children[i], dy: 8, delayMs: Math.Min(i, 8) * 35, ms: 460);
        for (var i = 0; i < Buttons.Children.Count; i++)
            Motion.RiseIn(Buttons.Children[i], dy: 6, delayMs: 80 + i * 40, ms: 420);
        Scroll.ScrollToTop();
    }

    private void SetOrb(Brush color, bool breathing)
    {
        Dot.Fill = color;
        Halo.Fill = color;
        if (breathing)
        {
            var grow = new DoubleAnimation(0.7, 1.7, TimeSpan.FromMilliseconds(1100))
            {
                AutoReverse = true,
                RepeatBehavior = RepeatBehavior.Forever,
                EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut },
            };
            HaloScale.BeginAnimation(ScaleTransform.ScaleXProperty, grow);
            HaloScale.BeginAnimation(ScaleTransform.ScaleYProperty, grow);
            Halo.BeginAnimation(OpacityProperty, new DoubleAnimation(0.45, 0.08, TimeSpan.FromMilliseconds(1100))
            {
                AutoReverse = true,
                RepeatBehavior = RepeatBehavior.Forever,
                EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut },
            });
        }
        else
        {
            // The breath settles into a still glow with one small spring.
            var settle = new DoubleAnimation(1.25, TimeSpan.FromMilliseconds(600)) { EasingFunction = new Spring { Damping = 0.5 } };
            HaloScale.BeginAnimation(ScaleTransform.ScaleXProperty, settle);
            HaloScale.BeginAnimation(ScaleTransform.ScaleYProperty, settle);
            Halo.BeginAnimation(OpacityProperty, new DoubleAnimation(0.22, TimeSpan.FromMilliseconds(300)));
        }
    }

    /// <summary>The waiting dots: a slow wave, each a beat after the last.</summary>
    private void Wave(bool on)
    {
        var dots = new[] { Dot1, Dot2, Dot3 };
        for (var i = 0; i < dots.Length; i++)
        {
            var move = dots[i].RenderTransform as TranslateTransform ?? new TranslateTransform();
            dots[i].RenderTransform = move;
            if (!on)
            {
                move.BeginAnimation(TranslateTransform.YProperty, null);
                dots[i].BeginAnimation(OpacityProperty, null);
                continue;
            }
            var begin = TimeSpan.FromMilliseconds(i * 140);
            var ease = new SineEase { EasingMode = EasingMode.EaseInOut };
            move.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(0, -4, TimeSpan.FromMilliseconds(420))
                { AutoReverse = true, RepeatBehavior = RepeatBehavior.Forever, BeginTime = begin, EasingFunction = ease });
            dots[i].BeginAnimation(OpacityProperty, new DoubleAnimation(0.35, 1, TimeSpan.FromMilliseconds(420))
                { AutoReverse = true, RepeatBehavior = RepeatBehavior.Forever, BeginTime = begin, EasingFunction = ease });
        }
    }

    /// <summary>The blinking caret at the end of a streaming answer.</summary>
    private FrameworkElement Caret()
    {
        var caret = new Border { Width = 2, Height = 15, Margin = new Thickness(2, 0, 0, 0), CornerRadius = new CornerRadius(1), Background = Resource("Ember") };
        caret.BeginAnimation(OpacityProperty, new DoubleAnimation(1, 0.15, TimeSpan.FromMilliseconds(520))
            { AutoReverse = true, RepeatBehavior = RepeatBehavior.Forever, EasingFunction = new SineEase() });
        return caret;
    }

    private void CloseSoftly()
    {
        if (_closing || IsClosed)
            return;
        _closing = true;
        Motion.SinkOut(Frame, Close);
    }

    private void OnKey(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Escape)
        {
            CloseSoftly();
            e.Handled = true;
        }
    }

    /// <summary>
    /// As tall as what it shows (up to the work area): a short answer is a
    /// short card, and a growing one eases taller rather than jumping.
    /// </summary>
    private void Fit(bool animate)
    {
        // The frame's margin is room for the shadow around the card.
        var room = Frame.Margin.Left + Frame.Margin.Right;
        Body.Measure(new System.Windows.Size(Width - room - 2, double.PositiveInfinity));
        var want = Math.Min(Math.Ceiling(Body.DesiredSize.Height) + 2 + room, SystemParameters.WorkArea.Height - 24);
        if (Math.Abs(want - ActualHeight) < 1 && IsLoaded)
            return;
        if (!animate || !IsLoaded)
        {
            BeginAnimation(HeightProperty, null);
            Height = want;
            return;
        }
        var grow = new DoubleAnimation(want, TimeSpan.FromMilliseconds(240)) { EasingFunction = Motion.Out };
        grow.Completed += (_, _) =>
        {
            BeginAnimation(HeightProperty, null);
            Height = want;
        };
        BeginAnimation(HeightProperty, grow);
    }

    /// <summary>Fits once the new content has been laid out, and tells assistive tech it changed.</summary>
    private void Refit() =>
        Dispatcher.BeginInvoke(System.Windows.Threading.DispatcherPriority.Loaded, () =>
        {
            Fit(animate: true);
            Announce();
        });

    /// <summary>
    /// WPF keeps the accessibility tree it built the first time a screen reader
    /// (or a test) looked, so a card read while "Thinking…" would stay that way:
    /// rebuild it, and let a screen reader speak the answer when it lands.
    /// </summary>
    private void Announce()
    {
        foreach (UIElement e in new UIElement[] { this, Scroll })
        {
            var peer = UIElementAutomationPeer.FromElement(e);
            peer?.ResetChildrenCache();
            peer?.RaiseAutomationEvent(AutomationEvents.StructureChanged);
        }
        if (_stream is null && Blocks.Children.Count > 0)
        {
            AutomationProperties.SetLiveSetting(Blocks.Children[0], AutomationLiveSetting.Polite);
            UIElementAutomationPeer.CreatePeerForElement(Blocks.Children[0])?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
        }
    }

    /// <summary>Above the tray, in the corner of the work area, growing upward as the answer grows.</summary>
    private void Place()
    {
        // The card sits 16 from the corner; the shadow's margin may hang past it.
        var area = SystemParameters.WorkArea;
        Left = area.Right - ActualWidth + Frame.Margin.Right - 16;
        Top = area.Bottom - ActualHeight + Frame.Margin.Bottom - 16;
    }

    // ---- building blocks ----

    private Brush Resource(string key) => (Brush)FindResource(key);

    private void AddButtons(params Button[] buttons)
    {
        foreach (var b in buttons)
            Buttons.Children.Add(b);
    }

    private Button Pill(string text, SymbolRegular icon, bool primary = false)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal };
        content.Children.Add(new SymbolIcon { Symbol = icon, FontSize = 14, Margin = new Thickness(0, 0, 6, 0), VerticalAlignment = VerticalAlignment.Center });
        content.Children.Add(new TextBlock { Text = text, VerticalAlignment = VerticalAlignment.Center });
        var button = new Button { Content = content, Style = (Style)FindResource(primary ? "PrimaryPill" : "Pill") };
        AutomationProperties.SetName(button, text); // what a screen reader says
        return button;
    }

    private static void Copy(Button button, string text)
    {
        string said;
        if (string.IsNullOrEmpty(text))
            said = "Nothing to copy";
        else
        {
            try
            {
                Clipboard.SetDataObject(text, copy: true, retryTimes: 10, retryDelay: 50);
                said = "Copied";
            }
            catch (ExternalException)
            {
                said = "Clipboard busy";
            }
        }
        var before = button.Content;
        var content = new StackPanel { Orientation = Orientation.Horizontal };
        content.Children.Add(new SymbolIcon
        {
            Symbol = said == "Copied" ? SymbolRegular.Checkmark24 : SymbolRegular.Warning24,
            FontSize = 14,
            Margin = new Thickness(0, 0, 6, 0),
            VerticalAlignment = VerticalAlignment.Center,
        });
        content.Children.Add(new TextBlock { Text = said, VerticalAlignment = VerticalAlignment.Center });
        button.Content = content;
        Motion.FadeIn(content, 160);
        var back = new System.Windows.Threading.DispatcherTimer { Interval = TimeSpan.FromSeconds(1.6) };
        back.Tick += (_, _) =>
        {
            back.Stop();
            button.Content = before;
            if (before is UIElement e)
                Motion.FadeIn(e, 160);
        };
        back.Start();
    }

    private FrameworkElement Render(Block block)
    {
        var text = Text(block.Spans);
        switch (block.Kind)
        {
            case BlockKind.Heading:
                text.FontSize = 15;
                text.FontWeight = FontWeights.SemiBold;
                text.Margin = new Thickness(0, 4, 0, 6);
                return text;
            case BlockKind.Code:
                return CodeWell(block.Spans.Single().Text, Resource("Signal"));
            case BlockKind.Bullet:
            case BlockKind.Numbered:
                var row = new Grid { Margin = new Thickness(2, 0, 0, 5) };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(18) });
                row.ColumnDefinitions.Add(new ColumnDefinition());
                FrameworkElement mark = block.Kind == BlockKind.Bullet
                    ? new Ellipse { Width = 5, Height = 5, Fill = Resource("Ember"), Opacity = 0.85, Margin = new Thickness(1, 8, 0, 0), VerticalAlignment = VerticalAlignment.Top, HorizontalAlignment = HorizontalAlignment.Left }
                    : new TextBlock { Text = block.Marker + ".", Style = (Style)FindResource("Body"), Foreground = Resource("Subtext") };
                Grid.SetColumn(text, 1);
                text.Margin = new Thickness(0);
                row.Children.Add(mark);
                row.Children.Add(text);
                return row;
            default:
                return text;
        }
    }

    private TextBlock Text(IReadOnlyList<Span> spans)
    {
        var t = new TextBlock { Style = (Style)FindResource("Body"), Margin = new Thickness(0, 0, 0, 8) };
        // Screen readers and the end-to-end test find the answer by this.
        AutomationProperties.SetAutomationId(t, "Answer");
        foreach (var s in spans)
        {
            var run = new Run(s.Text);
            if (s.Bold)
                run.FontWeight = FontWeights.SemiBold;
            if (s.Italic)
                run.FontStyle = FontStyles.Italic;
            if (s.Code)
            {
                run.FontFamily = (System.Windows.Media.FontFamily)FindResource("MonoFont");
                run.FontSize = 13;
                run.Foreground = Resource("Signal");
                run.Background = Resource("Well");
            }
            t.Inlines.Add(run);
        }
        return t;
    }

    private Border CodeWell(string code, Brush color) => new()
    {
        Background = Resource("CodeWell"),
        BorderBrush = Resource("Hairline"),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(6),
        Padding = new Thickness(11, 8, 11, 8),
        Margin = new Thickness(0, 2, 0, 10),
        Child = new TextBlock
        {
            Text = code,
            FontFamily = (System.Windows.Media.FontFamily)FindResource("MonoFont"),
            FontSize = 12.5,
            LineHeight = 18,
            Foreground = color,
            TextWrapping = TextWrapping.Wrap,
        },
    };
}
