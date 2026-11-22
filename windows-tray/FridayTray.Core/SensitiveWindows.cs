namespace FridayTray.Core;

/// <summary>
/// Mail and chat apps (as `friday_sensitive_window` on Linux): their text is
/// mostly someone else's words, so the ask box says where it goes. The
/// built-in names match the app only ("element" in a page title is no chat
/// app); the person's own list (`sensitive-windows` in the config folder)
/// adds lines matched against the app or the title, and "!word" drops one.
/// </summary>
public static class SensitiveWindows
{
    public static readonly string[] Defaults =
    [
        "thunderbird", "outlook", "olk", "signal", "telegram", "discord", "vesktop",
        "slack", "whatsapp", "element", "teams",
    ];

    /// <summary>What made the window a mail or chat app, or null.</summary>
    public static string? Match(string? app, string? title, string? ownList)
    {
        var own = (ownList ?? "").Split('\n').Select(l => l.Trim().ToLowerInvariant())
            .Where(l => l.Length > 0 && !l.StartsWith('#')).ToList();
        var dropped = own.Where(l => l.StartsWith('!')).Select(l => l[1..].Trim()).ToHashSet();
        var inApp = (app ?? "").ToLowerInvariant();
        var anywhere = $"{app} {title}".ToLowerInvariant();
        return Defaults.Where(d => !dropped.Contains(d)).FirstOrDefault(inApp.Contains)
            ?? own.Where(l => !l.StartsWith('!')).FirstOrDefault(anywhere.Contains);
    }

    public static string? Match(Paths paths, string? app, string? title)
    {
        string? own = null;
        try { own = File.ReadAllText(paths.SensitiveWindowsFile); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        return Match(app, title, own);
    }
}
