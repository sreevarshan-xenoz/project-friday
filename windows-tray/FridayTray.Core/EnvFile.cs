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

    /// <summary>
    /// The file after choosing <paramref name="provider"/>: other settings are
    /// kept, every provider key and FRIDAY_CONTEXT_PROVIDER are replaced.
    /// </summary>
    public static string WithProvider(string existing, Provider provider, string key)
    {
        var lines = new List<string> { "# FRIDAY AI provider credentials (FRIDAY tray: AI provider)." };
        foreach (var raw in existing.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            var t = line.Trim();
            if (t.Length == 0 || t.StartsWith('#'))
                continue;
            var name = t.Split('=', 2)[0].Trim();
            if (name.EndsWith("_API_KEY", StringComparison.Ordinal) || name == "FRIDAY_CONTEXT_PROVIDER")
                continue;
            lines.Add(line);
        }
        if (key.Length > 0)
            lines.Add($"{provider.KeyVar}={key}");
        if (provider.Vision)
            lines.Add($"FRIDAY_CONTEXT_PROVIDER={provider.Id}");
        return string.Join('\n', lines) + "\n";
    }
}
