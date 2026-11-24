namespace FridayTray.Core;

/// <summary>
/// When each clipboard text was last copied, so an old copy isn't taken as
/// context for a new question. Only a hash of each text is kept, in memory.
/// A text the tray never saw change (copied before it started) has no age.
/// </summary>
public sealed class ClipboardAge
{
    public static readonly TimeSpan Limit = TimeSpan.FromMinutes(10);
    private const int Keep = 50;
    private readonly Dictionary<string, DateTime> _first = new();
    private readonly Queue<string> _order = new();

    private static string Hash(string text) =>
        Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(text)));

    /// <summary>The person copied <paramref name="text"/> now (copying it again counts as new).</summary>
    public void Seen(string? text, DateTime now)
    {
        if (string.IsNullOrEmpty(text))
            return;
        var h = Hash(text);
        var known = _first.ContainsKey(h);
        _first[h] = now;
        if (known)
            return;
        _order.Enqueue(h);
        while (_order.Count > Keep)
            _first.Remove(_order.Dequeue());
    }

    /// <summary>How long ago <paramref name="text"/> was copied, when it is known.</summary>
    public TimeSpan? Age(string text, DateTime now) =>
        _first.TryGetValue(Hash(text), out var t) ? now - t : null;

    /// <summary>What to say instead of using an old copy, or null when it is fresh or its age unknown.</summary>
    public string? TooOld(string text, DateTime now)
    {
        if (Age(text, now) is not { } age || age <= Limit)
            return null;
        var mins = (int)age.TotalMinutes;
        return $"The clipboard was copied {mins} minutes ago, so I didn't use it. Copy it again, or select the text you mean.";
    }
}
