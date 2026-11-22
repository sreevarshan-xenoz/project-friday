namespace FridayTray.Core;

/// <summary>
/// Mail and chat apps (as `friday_sensitive_window` on Linux): their text is
/// mostly someone else's words, so the ask box says where it goes. Matched
/// like <see cref="PrivateWindows"/>; the person's own list
/// (`sensitive-windows` in the config folder) adds lines, "!word" drops one.
/// </summary>
public static class SensitiveWindows
{
    public static readonly string[] Defaults =
    [
        "thunderbird", "outlook", "signal", "telegram", "discord", "vesktop",
        "slack", "whatsapp", "element", "teams",
    ];

    /// <summary>What made the window a mail or chat app, or null.</summary>
    public static string? Match(string? app, string? title, string? ownList) =>
        PrivateWindows.Match(Defaults, app, title, ownList);

    public static string? Match(Paths paths, string? app, string? title)
    {
        string? own = null;
        try { own = File.ReadAllText(paths.SensitiveWindowsFile); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        return Match(app, title, own);
    }
}
