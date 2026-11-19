using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Interop;
using Wpf.Ui.Controls;
using MenuItem = System.Windows.Controls.MenuItem;

namespace FridayTray.Fluent;

/// <summary>One line of the tray menu; <see cref="Checked"/> makes it a switch.</summary>
internal sealed record MenuEntry(string Text, SymbolRegular Icon, Action Run, string? Hint = null, bool? Checked = null, bool Bold = false);

/// <summary>
/// The tray icon's menu, in the same Fluent look as the windows: built fresh
/// each time it opens (so every check mark is current) and shown at the
/// pointer. A null entry is a separator.
/// </summary>
internal static class TrayMenu
{
    /// <param name="at">where to open it (screen units); the pointer when null</param>
    public static ContextMenu Open(IReadOnlyList<MenuEntry?> entries, System.Windows.Point? at = null)
    {
        var menu = new ContextMenu { Placement = PlacementMode.MousePoint, StaysOpen = false };
        if (at is { } p)
        {
            menu.Placement = PlacementMode.AbsolutePoint;
            menu.HorizontalOffset = p.X;
            menu.VerticalOffset = p.Y;
        }
        foreach (var e in entries)
        {
            if (e is null)
            {
                menu.Items.Add(new Separator());
                continue;
            }
            var item = new MenuItem
            {
                Header = e.Text,
                InputGestureText = e.Hint ?? "",
                FontWeight = e.Bold ? FontWeights.SemiBold : FontWeights.Normal,
            };
            // A switch shows its check mark where the others show an icon.
            if (e.Checked is { } on)
            {
                item.IsCheckable = true;
                item.IsChecked = on;
            }
            else
                item.Icon = new SymbolIcon { Symbol = e.Icon };
            var run = e.Run;
            item.Click += (_, _) => run();
            menu.Items.Add(item);
        }
        // A tray app isn't in front, so its menu wouldn't close on a click
        // elsewhere: bring the menu's own window forward once it is open.
        menu.Opened += (_, _) =>
        {
            if (PresentationSource.FromVisual(menu) is HwndSource source)
                Native.SetForegroundWindow(source.Handle);
        };
        menu.IsOpen = true;
        return menu;
    }
}
