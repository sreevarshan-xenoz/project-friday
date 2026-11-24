namespace FridayTray.Core;

/// <summary>
/// Asks that fit the app in front, offered first in the ask box (as on
/// Linux). Each is about the window, so picking one asks about it with the
/// usual screen consent.
/// </summary>
public static class AppSuggestions
{
    public sealed record Suggestion(string Label, string Question);

    private static readonly string[] Readers =
        ["firefox", "chrome", "msedge", "brave", "zen", "vivaldi", "opera", "librewolf", "acrord32", "acrobat", "sumatrapdf"];
    private static readonly string[] Mail = ["thunderbird", "outlook", "olk", "hxoutlook"];
    private static readonly string[] Chat =
        ["slack", "discord", "telegram", "signal", "element", "whatsapp", "teams", "ms-teams"];

    public static IReadOnlyList<Suggestion> For(string? app)
    {
        var a = (app ?? "").Trim().ToLowerInvariant();
        if (a.EndsWith(".exe", StringComparison.Ordinal))
            a = a[..^4];
        if (Readers.Contains(a))
            return [new("Summarize this page", "Summarize this page")];
        if (Mail.Contains(a))
            return
            [
                new("Summarize this email", "Summarize this email"),
                new("Draft a reply", "Draft a short reply to this email, in my voice, ready to paste"),
            ];
        if (Chat.Contains(a))
            return
            [
                new("Draft a reply", "Draft a short reply to the last message here, ready to paste"),
                new("Catch me up", "Summarize what this conversation is about and what is asked of me"),
            ];
        return [];
    }

    /// <summary>The suggestion whose label was picked, if <paramref name="question"/> is one.</summary>
    public static Suggestion? Picked(IReadOnlyList<Suggestion> offered, string question)
    {
        var q = question.Trim();
        // The label as shown (with what goes after " · "), or typed without that.
        return offered.FirstOrDefault(s => s.Label == q || s.Label.Split(" · ")[0] == q);
    }
}
