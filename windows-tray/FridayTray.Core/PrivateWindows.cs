namespace FridayTray.Core;

/// <summary>
/// Windows whose screenshot or selected text never leaves the PC (as
/// `friday_private_window` on Linux): matched, case aside, against the app
/// and the window title. The person's own list (`private-windows` in the
/// config folder) adds lines; "!word" there drops a built-in one.
/// </summary>
public static class PrivateWindows
{
    public static readonly string[] Defaults =
    [
        "keepass", "bitwarden", "1password", "enpass", "proton pass", "dashlane", "lastpass",
        "credential manager", "private browsing", "incognito", "inprivate", "bank", "paypal",
    ];

    /// <summary>What made the window private, or null.</summary>
    public static string? Match(string? app, string? title, string? ownList) =>
        Match(Defaults, app, title, ownList);

    internal static string? Match(string[] defaults, string? app, string? title, string? ownList)
    {
        var hay = $"{app} {title}".ToLowerInvariant();
        var own = (ownList ?? "").Split('\n').Select(l => l.Trim().ToLowerInvariant())
            .Where(l => l.Length > 0 && !l.StartsWith('#')).ToList();
        var dropped = own.Where(l => l.StartsWith('!')).Select(l => l[1..].Trim()).ToHashSet();
        return defaults.Where(d => !dropped.Contains(d))
            .Concat(own.Where(l => !l.StartsWith('!')))
            .FirstOrDefault(pattern => hay.Contains(pattern));
    }

    public static string? Match(Paths paths, string? app, string? title)
    {
        string? own = null;
        try { own = File.ReadAllText(paths.PrivateWindowsFile); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        return Match(app, title, own);
    }
}
