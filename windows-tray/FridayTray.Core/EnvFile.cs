namespace FridayTray.Core;

/// <summary>
/// The AI provider file (`friday.env`): `KEY=value` lines, `#` comments. The
/// tray passes these to the daemon's environment. Same format and rules as
/// `friday setup-ai` on Linux. Values are never logged or shown.
/// </summary>
public static class EnvFile
{
    public static Dictionary<string, string> Parse(string text)
    {
        var vars = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#'))
                continue;
            var eq = line.IndexOf('=');
            if (eq <= 0)
                continue;
            var name = line[..eq].Trim();
            var value = line[(eq + 1)..].Trim();
            if (value.Length >= 2 && (value[0] == '"' || value[0] == '\'') && value[^1] == value[0])
                value = value[1..^1];
            vars[name] = value;
        }
        return vars;
    }

    public static Dictionary<string, string> Load(string path) =>
        File.Exists(path) ? Parse(File.ReadAllText(path)) : new(StringComparer.Ordinal);

    /// <summary>Whether the file holds a key for a provider other than <paramref name="provider"/>.</summary>
    public static bool HasOtherKeys(string existing, Provider provider) =>
        Parse(existing).Keys.Any(k => k.EndsWith("_API_KEY", StringComparison.Ordinal) && k != provider.KeyVar);

    /// <summary>
    /// The file after choosing <paramref name="provider"/>: other settings are
    /// kept. Every provider key and FRIDAY_CONTEXT_PROVIDER are replaced, or,
    /// with <paramref name="keepOthers"/>, only this provider's key (FRIDAY
    /// falls back to the others) and the screen provider only when this one
    /// can take screen questions. Same rules as `friday setup-ai --keep-others`.
    /// </summary>
    public static string WithProvider(string existing, Provider provider, string key, bool keepOthers = false)
    {
        var lines = new List<string> { "# FRIDAY AI provider credentials (FRIDAY tray: AI provider)." };
        foreach (var raw in existing.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            var t = line.Trim();
            if (t.Length == 0 || t.StartsWith('#'))
                continue;
            var name = t.Split('=', 2)[0].Trim();
            var drop = keepOthers
                ? name == provider.KeyVar || (provider.Vision && name == "FRIDAY_CONTEXT_PROVIDER")
                : name.EndsWith("_API_KEY", StringComparison.Ordinal) || name == "FRIDAY_CONTEXT_PROVIDER";
            if (drop)
                continue;
            lines.Add(line);
        }
        if (key.Length > 0)
            lines.Add($"{provider.KeyVar}={key}");
        if (provider.Vision)
            lines.Add($"FRIDAY_CONTEXT_PROVIDER={provider.Id}");
        return string.Join('\n', lines) + "\n";
    }

    /// <summary>
    /// The file with <paramref name="name"/> set to <paramref name="value"/>
    /// (or removed, when null); every other line stays as it was.
    /// </summary>
    public static string WithSetting(string existing, string name, string? value)
    {
        var lines = existing.Split('\n').Select(l => l.TrimEnd('\r'))
            .Where(l => l.Trim().Split('=', 2)[0].Trim() != name)
            .ToList();
        while (lines.Count > 0 && lines[^1].Trim().Length == 0)
            lines.RemoveAt(lines.Count - 1);
        if (value != null)
            lines.Add($"{name}={value}");
        return lines.Count == 0 ? "" : string.Join('\n', lines) + "\n";
    }
}
