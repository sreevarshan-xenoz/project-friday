using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>
/// "Close this window", "move this app": the window in front at the keypress,
/// named with the ask so switching windows while FRIDAY thinks can't change
/// what it acts on (as <c>friday ask</c> does on Linux). The daemon reads
/// exactly this shape, outside quoted text only.
/// </summary>
public static partial class ThisWindow
{
    [GeneratedRegex(@"(^|[^a-z])(this|current) (window|app|tab)([^a-z]|$)", RegexOptions.IgnoreCase)]
    private static partial Regex Names();

    public static bool Matches(string question) => Names().IsMatch(question);

    /// <summary>The block for the request, or null when the app's name isn't a plain one.</summary>
    public static string? Block(string? app, string? title)
    {
        var a = new string((app ?? "").Where(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '_' or '-').ToArray());
        if (a.Length == 0)
            return null;
        if (a.Length > 60)
            a = a[..60];
        var t = new string((title ?? "").Where(c => !char.IsControl(c) && c != '"').ToArray());
        if (t.Length > 120)
            t = t[..120];
        return $"\n\n[window in front when asked]\n\"this window\" is {a} (title: \"{t}\"), even if another is in front now.";
    }
}
