using System.Windows;
using System.Windows.Controls;
using FridayTray.Core;
using Wpf.Ui.Controls;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// What left this PC: each request FRIDAY sent to an AI provider, newest
/// first. Never the question or the answer: which provider and model, what
/// kind of thing went (text, a screenshot) and how much, how many secrets were
/// masked first, and how it went.
/// </summary>
internal sealed class EgressWindow : CardWindow
{
    public EgressWindow(Paths paths) : base("FRIDAY · What left this PC", "What left this PC", 640)
    {
        var entries = EgressLog.ReadNewestFirst(paths.EgressFile);
        Body.Children.Add(new TextBlock
        {
            Text = EgressLog.FormatSummary(entries, DateTime.UtcNow),
            FontSize = 14,
            FontWeight = FontWeights.SemiBold,
            Foreground = R("Ember"),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 0, 0, 12),
        });

        if (entries.Count == 0)
        {
            // The heading already says nothing was sent; say what will show here.
            var empty = new StackPanel { Orientation = Orientation.Horizontal };
            empty.Children.Add(new SymbolIcon { Symbol = SymbolRegular.ArrowUpload24, FontSize = 16, Foreground = R("Subtext"), Margin = new Thickness(0, 1, 10, 0), VerticalAlignment = VerticalAlignment.Top });
            empty.Children.Add(new TextBlock
            {
                Text = "Each question FRIDAY sends to an AI provider will be listed here: which one, what kind (text or a screenshot), how much, and how many secrets were masked first.",
                FontSize = 12.5,
                LineHeight = 18,
                Foreground = R("Subtext"),
                TextWrapping = TextWrapping.Wrap,
                MaxWidth = 540,
            });
            Body.Children.Add(Well(empty, new Thickness(14, 12, 14, 12)));
        }
        else
        {
            var rows = new StackPanel();
            rows.Children.Add(Row("When", "Provider · model", "What", "Masked", "Outcome", header: true));
            foreach (var e in entries)
            {
                rows.Children.Add(new Border { Height = 1, Background = R("Hairline"), Opacity = 0.6 });
                rows.Children.Add(Row(EgressLog.FormatLocalTime(e.Ts), $"{EgressLog.FormatProvider(e.Provider)} · {e.Model}",
                    EgressLog.FormatWhat(e.TextChars, e.ImageBytes), (e.Masked ?? 0).ToString(), e.Outcome));
            }
            Body.Children.Add(Well(new ScrollViewer { Content = rows, MaxHeight = 380, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }, new Thickness(14, 4, 6, 4)));
        }

        var close = Pill("Close", primary: true);
        close.IsDefault = true;
        close.IsCancel = true;
        close.Click += (_, _) => Close();
        Buttons.Children.Add(close);
    }

    private Grid Row(string when, string who, string what, string masked, string outcome, bool header = false)
    {
        var g = new Grid { Margin = new Thickness(0, 8, 8, 8) };
        foreach (var w in new[] { 150.0, 0, 140, 56, 64 })
            g.ColumnDefinitions.Add(new ColumnDefinition { Width = w == 0 ? new GridLength(1, GridUnitType.Star) : new GridLength(w) });
        string[] cells = [when, who, what, masked, outcome];
        for (var i = 0; i < cells.Length; i++)
        {
            var t = new TextBlock
            {
                Text = cells[i],
                FontSize = header ? 11 : 12.5,
                FontWeight = header ? FontWeights.SemiBold : FontWeights.Normal,
                Foreground = R(header ? "Faint" : i == 4 && cells[i] != "ok" ? "Warn" : "Text"),
                TextTrimming = TextTrimming.CharacterEllipsis,
                HorizontalAlignment = i == 3 ? HorizontalAlignment.Right : HorizontalAlignment.Left,
                Margin = new Thickness(0, 0, 10, 0),
            };
            Grid.SetColumn(t, i);
            g.Children.Add(t);
        }
        return g;
    }
}
