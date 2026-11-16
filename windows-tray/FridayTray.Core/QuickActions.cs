namespace FridayTray.Core;

/// <summary>
/// What the ask box offers when text is selected: four built-in actions, then
/// the user's own prompts (one per line in <c>prompts</c> in the config
/// directory; blank lines and lines starting with # are skipped).
/// </summary>
public static class QuickActions
{
    public static readonly IReadOnlyList<string> Defaults =
    [
        "Summarize this",
        "Explain this simply",
        "Translate this to English",
        "Fix the grammar and spelling; reply with only the corrected text",
    ];

    /// <summary>The built-in actions followed by the prompts in <paramref name="promptsFile"/>, if it can be read.</summary>
    public static IReadOnlyList<string> All(string promptsFile)
    {
        var list = new List<string>(Defaults);
        try
        {
            foreach (var raw in File.ReadLines(promptsFile))
            {
                var line = raw.Trim();
                if (line.Length > 0 && !line.StartsWith('#') && !list.Contains(line))
                    list.Add(line);
            }
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
        return list;
    }
}
