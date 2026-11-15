namespace FridayTray.Core;

/// <summary>
/// Which app a selection came from, told to the model with the question
/// (`friday app-context` on Linux): the program and its window title. Off with
/// a marker file in the config folder, and on by default.
/// </summary>
public static class SourceApp
{
    public const int MaxTitleChars = 120;

    public static bool Enabled(Paths paths) => !File.Exists(paths.NoAppContextFile);

    public static void SetEnabled(Paths paths, bool on)
    {
        if (on)
        {
            try { File.Delete(paths.NoAppContextFile); } catch (IOException) { }
            return;
        }
        Directory.CreateDirectory(paths.ConfigDir);
        File.WriteAllText(paths.NoAppContextFile, "");
    }

    /// <summary>The sentence that goes after the question, or null when the app is unknown.</summary>
    public static string? Line(string? app, string? title)
    {
        app = Clean(app, 80);
        if (app.Length == 0)
            return null;
        title = Clean(title, MaxTitleChars);
        return title.Length > 0
            ? $"The text came from the app \"{app}\" (window title: \"{title}\")."
            : $"The text came from the app \"{app}\".";
    }

    /// <summary>One line, no quotes (they would end the sentence's own), clipped.</summary>
    private static string Clean(string? text, int max)
    {
        var one = (text ?? "").Replace("\"", "").ReplaceLineEndings(" ").Trim();
        return one.Length > max ? one[..max].TrimEnd() : one;
    }

    /// <summary>The request with the app sentence on its own line.</summary>
    public static string Append(string request, string? line) =>
        string.IsNullOrEmpty(line) ? request : request + "\n" + line;
}
