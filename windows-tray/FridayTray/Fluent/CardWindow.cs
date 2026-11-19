using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Effects;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Brushes = System.Windows.Media.Brushes;
using Button = System.Windows.Controls.Button;
using Color = System.Windows.Media.Color;
using FontFamily = System.Windows.Media.FontFamily;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using KeyEventArgs = System.Windows.Input.KeyEventArgs;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// The frame the smaller windows share (the AI provider, accounts, what left
/// this PC): a rounded card with a soft shadow, a heading, a close button,
/// the window's own content and a row of buttons. Esc closes it, the card
/// drags it, and it rises in when it opens.
/// </summary>
internal abstract class CardWindow : Window
{
    /// <summary>Where a window puts its content.</summary>
    protected readonly StackPanel Body = new();

    /// <summary>Its buttons, right-aligned under the content.</summary>
    protected readonly StackPanel Buttons = new() { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 20, 0, 0) };

    /// <summary>Closed; the tray makes a new one when asked again.</summary>
    public bool IsClosed { get; private set; }

    protected CardWindow(string title, string heading, double width)
    {
        Title = title;
        WindowStyle = WindowStyle.None;
        AllowsTransparency = true;
        Background = Brushes.Transparent;
        ResizeMode = ResizeMode.NoResize;
        SizeToContent = SizeToContent.Height;
        Width = width + 48;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;

        var close = new Button { Style = (Style)FindResource("IconButton"), Content = new SymbolIcon { Symbol = SymbolRegular.Dismiss24, FontSize = 13 }, ToolTip = "Close (Esc)" };
        AutomationProperties.SetName(close, "Close");
        close.Click += (_, _) => Close();
        var head = new Grid { Margin = new Thickness(0, 0, 0, 14) };
        head.ColumnDefinitions.Add(new ColumnDefinition());
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.Children.Add(new TextBlock
        {
            Text = heading,
            FontFamily = (FontFamily)FindResource("DisplayFont"),
            FontSize = 20,
            FontWeight = FontWeights.SemiBold,
            Foreground = R("Text"),
            VerticalAlignment = VerticalAlignment.Center,
        });
        Grid.SetColumn(close, 1);
        head.Children.Add(close);

        var content = new StackPanel { Margin = new Thickness(26, 20, 22, 22) };
        content.Children.Add(head);
        content.Children.Add(Body);
        content.Children.Add(Buttons);

        var card = new Border
        {
            Background = R("Glass"),
            BorderBrush = R("Hairline"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(14),
            Child = new Grid { Children = { new Border { Background = R("EmberWash"), CornerRadius = new CornerRadius(14), IsHitTestVisible = false }, content } },
        };
        // The shadow is its own layer, so the card's text stays crisp.
        var shadow = new Border
        {
            CornerRadius = new CornerRadius(14),
            Background = new SolidColorBrush(Color.FromRgb(0x0D, 0x0F, 0x13)),
            IsHitTestVisible = false,
            Effect = new DropShadowEffect { BlurRadius = 34, ShadowDepth = 10, Direction = 270, Opacity = 0.5, Color = Colors.Black },
        };
        var frame = new Grid { Margin = new Thickness(24), Children = { shadow, card } };
        Content = frame;

        card.MouseLeftButtonDown += (_, e) => { if (e.ButtonState == MouseButtonState.Pressed) DragMove(); };
        KeyDown += (_, e) => { if (e.Key == Key.Escape) Close(); };
        Closed += (_, _) => IsClosed = true;
        Loaded += (_, _) => Motion.RiseIn(frame, dy: 12, ms: 560, fromScale: 0.97);
    }

    protected Brush R(string key) => (Brush)FindResource(key);

    protected Button Pill(string text, SymbolRegular? icon = null, bool primary = false)
    {
        object content = text;
        if (icon is { } i)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal };
            row.Children.Add(new SymbolIcon { Symbol = i, FontSize = 14, Margin = new Thickness(0, 0, 7, 0), VerticalAlignment = VerticalAlignment.Center });
            row.Children.Add(new TextBlock { Text = text, VerticalAlignment = VerticalAlignment.Center });
            content = row;
        }
        var b = new Button { Content = content, Style = (Style)FindResource(primary ? "PrimaryPill" : "Pill") };
        AutomationProperties.SetName(b, text);
        return b;
    }

    protected TextBlock Note(string text, string color = "Subtext") => new()
    {
        Text = text,
        FontSize = 13,
        LineHeight = 19,
        Foreground = R(color),
        TextWrapping = TextWrapping.Wrap,
        Margin = new Thickness(0, 0, 0, 12),
    };

    /// <summary>A soft rounded panel for a group of things.</summary>
    protected Border Well(UIElement child, Thickness? padding = null) => new()
    {
        Background = R("Well"),
        BorderBrush = R("Hairline"),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(10),
        Padding = padding ?? new Thickness(14, 10, 14, 10),
        Margin = new Thickness(0, 0, 0, 12),
        Child = child,
    };
}
