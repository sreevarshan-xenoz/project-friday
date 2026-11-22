namespace FridayTray.Core;

using System.Text.Json;
using System.Text.RegularExpressions;

/// <summary>
/// The folders coding agents work in, for the ask box's folder chip: a request
/// that names an agent but no folder runs in the folder that agent last used
/// (the daemon's agents\last-folder.json), so the box says which before Enter.
/// </summary>
public sealed record AgentFolders(IReadOnlyDictionary<string, string> Last, IReadOnlyList<string> Known, string Home)
{
    /// <summary>Most folders offered when the chip is clicked.</summary>
    public const int MaxKnown = 6;

    private static readonly Regex AgentName = new(@"\b(opencode|agy|antigravity|claude[ -]code)\b",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    // ~/x, C:\x, \\server\x, /x, or code/site (the daemon reads that from the
    // home folder): the request already says where.
    private static readonly Regex FolderName = new(@"~[\\/]|\b[A-Za-z]:\\|\\\\\w|(^|\s)(\.{1,2}[\\/]|/|\w[\w.-]*[\\/])\w", RegexOptions.Compiled);

    public static AgentFolders Load(string agentsDir, string home)
    {
        var last = new Dictionary<string, string>(StringComparer.Ordinal);
        try
        {
            using var doc = JsonDocument.Parse(File.ReadAllText(Path.Combine(agentsDir, "last-folder.json")));
            foreach (var p in doc.RootElement.EnumerateObject())
                if (p.Value.ValueKind == JsonValueKind.String && p.Value.GetString() is { Length: > 0 } f)
                    last[p.Name] = f;
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException or InvalidOperationException)
        {
        }
        IReadOnlyList<AgentRun> runs;
        try
        {
            runs = AgentRuns.Load(agentsDir);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            runs = [];
        }
        var known = runs.Select(r => r.Folder).Concat(last.Values)
            .Where(f => f.Length > 0)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Take(MaxKnown)
            .ToList();
        return new AgentFolders(last, known, home);
    }

    /// <summary>The agent a request names, as the daemon spells it, or null.</summary>
    public static string? NamedAgent(string text)
    {
        var m = AgentName.Match(text);
        if (!m.Success)
            return null;
        return m.Value.ToLowerInvariant() switch
        {
            "agy" or "antigravity" => "agy",
            "claude code" or "claude-code" => "claude",
            _ => "opencode",
        };
    }

    public static bool NamesFolder(string text) => FolderName.IsMatch(text);

    /// <summary>What the chip says for <paramref name="text"/>, or null for no chip.</summary>
    public string? Chip(string text)
    {
        if (NamedAgent(text) is not { } agent || NamesFolder(text))
            return null;
        var name = agent == "claude" ? "Claude Code" : agent;
        return Last.TryGetValue(agent, out var folder)
            ? $"{name} works in {Tilde(folder)}"
            : $"Say which folder {name} works in, like ~{Path.DirectorySeparatorChar}code{Path.DirectorySeparatorChar}site";
    }

    /// <summary><paramref name="folder"/> with ~ for the home folder.</summary>
    public string Tilde(string folder)
    {
        var home = Home.TrimEnd('\\', '/');
        if (home.Length > 0 && folder.StartsWith(home, StringComparison.OrdinalIgnoreCase)
            && (folder.Length == home.Length || folder[home.Length] is '\\' or '/'))
            return "~" + folder[home.Length..];
        return folder;
    }
}
