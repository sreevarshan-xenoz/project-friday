using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>
/// A question that leans on "this" or "that" with nothing saying what it is
/// ("explain this", "what does that mean", "is this right?"), as
/// `friday_vague_this` on Linux. "This page", "this error in rust" or an
/// @mention name it. The ask box then asks which "this" instead of guessing.
/// </summary>
public static class VagueThis
{
    private static readonly Regex[] Patterns =
    [
        new(@"(^|[^a-z])(this|that|these|those)\s*(mean|means|say|says|do|does|for\s+me|please)?\s*[?.!]*$"),
        new(@"^(is|are)\s+(this|that|these|those)\s+(right|correct|ok|okay|true|safe|good|real|legit|a\s+scam)\s*[?.!]*$"),
        new(@"^(translate|summari[sz]e|explain|rewrite|fix|check|proofread|shorten|simplify)\s+(this|that|these|those|it)([^a-z]|$)"),
    ];

    public static bool Matches(string question)
    {
        var q = question.Trim().ToLowerInvariant();
        return q.Length is > 0 and <= 80 && !q.Contains('@') && Patterns.Any(p => p.IsMatch(q));
    }
}
