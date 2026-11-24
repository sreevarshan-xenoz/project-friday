namespace FridayTray.Core;

/// <summary>
/// How the person wants FRIDAY to answer in an app (<c>friday app-instructions</c>
/// on Linux): one "app: instruction" per line in <c>app-instructions</c> in the
/// config folder, matched against the program in front.
/// </summary>
public static class AppInstructions
{
    public const int MaxChars = 300;

    /// <summary>The same app: any case, with or without .exe, or a reverse-DNS id's last part.</summary>
    public static bool Same(string a, string b)
    {
        static string Bare(string s)
        {
            s = s.Trim().ToLowerInvariant();
            return s.EndsWith(".exe", StringComparison.Ordinal) ? s[..^4] : s;
        }
        a = Bare(a);
        b = Bare(b);
        if (a.Length == 0 || b.Length == 0)
            return false;
        static string Last(string s) => s[(s.LastIndexOf('.') + 1)..];
        return a == b || Last(a) == b || a == Last(b);
    }

    /// <summary>The instruction for <paramref name="app"/> in <paramref name="file"/>, if there is one.</summary>
    public static string? For(string file, string? app)
    {
        if (string.IsNullOrWhiteSpace(app))
            return null;
        try
        {
            foreach (var raw in File.ReadLines(file))
            {
                var line = raw.Trim();
                var colon = line.IndexOf(':');
                if (line.Length == 0 || line.StartsWith('#') || colon < 0)
                    continue;
                var name = string.Concat(line[..colon].Where(c => !char.IsWhiteSpace(c)));
                if (!Same(app, name))
                    continue;
                var rule = line[(colon + 1)..].Trim();
                if (rule.Length == 0)
                    return null;
                return rule.Length > MaxChars ? rule[..MaxChars] : rule;
            }
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
        return null;
    }

    /// <summary>The request with the app's instruction after it, as on Linux.</summary>
    public static string Append(string request, string app, string rule) =>
        $"{request}\n\n[app instructions]\nIn {app}, the person asked FRIDAY to follow this (their own setting):\n\"\"\"\n{rule}\n\"\"\"";
}
