namespace FridayTray.Core;

/// <summary>
/// "Rewrite: …" on selected text (as `friday ask --selection` on Linux): the
/// model's rewrite replaces the selection instead of opening an answer.
/// </summary>
public static class Rewrite
{
    public const string Prefix = "Rewrite: ";

    public static readonly IReadOnlyList<string> Actions =
    [
        "Rewrite: fix grammar and spelling",
        "Rewrite: make it shorter",
        "Rewrite: make it more formal",
        "Rewrite: make it friendlier",
        "Rewrite: translate to English",
    ];

    /// <summary>How to rewrite, when the question is a rewrite; otherwise null.</summary>
    public static string? How(string question) =>
        question.StartsWith(Prefix, StringComparison.Ordinal) && question.Length > Prefix.Length
            ? question[Prefix.Length..].Trim()
            : null;

    public static string Request(string how, string text) =>
        $"Rewrite the text below: {how}. Keep its language unless told to translate. " +
        $"Reply with only the rewritten text: no quotes, no notes, no introduction.\n\n[text to rewrite]\n\"\"\"\n{text}\n\"\"\"";

    /// <summary>The rewrite as the model may wrap it: a code fence or quotes around the whole answer dropped.</summary>
    public static string Clean(string answer)
    {
        var t = answer.Trim();
        if (t.StartsWith("```", StringComparison.Ordinal) && t.EndsWith("```", StringComparison.Ordinal) && t.Length > 6)
        {
            var lines = t.Split('\n');
            t = string.Join('\n', lines[1..^1]).Trim();
        }
        if (t.Length >= 2 && t[0] == '"' && t[^1] == '"' && !t[1..^1].Contains('"'))
            t = t[1..^1];
        return t;
    }
}
