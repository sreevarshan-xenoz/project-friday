namespace FridayTray.Core;

using System.Text.Json;

/// <summary>One thing FRIDAY did on the desktop, from actions.jsonl (the log
/// the daemon keeps beside state.json). Which action, with what, whether it
/// worked; never what was on the screen.</summary>
public sealed record ActionEntry(string Ts, string Verb, string Subject, bool Ok);

/// <summary>Reader for actions.jsonl, shown on the Desktop control page.</summary>
public static class ActionLog
{
    // The first plain value an action was given ("firefox", "2"), same order as the Linux shell.
    private static readonly string[] SubjectKeys = ["target", "app", "query", "command", "workspace", "desktop", "name", "label"];

    public static List<ActionEntry> Parse(string jsonl)
    {
        var entries = new List<ActionEntry>();
        if (string.IsNullOrWhiteSpace(jsonl))
            return entries;
        using var reader = new StringReader(jsonl);
        string? line;
        while ((line = reader.ReadLine()) != null)
        {
            if (line.Trim().Length == 0)
                continue;
            try
            {
                using var doc = JsonDocument.Parse(line);
                var root = doc.RootElement;
                if (!root.TryGetProperty("verb", out var verb) || verb.ValueKind != JsonValueKind.String)
                    continue;
                var subject = "";
                if (root.TryGetProperty("params", out var p) && p.ValueKind == JsonValueKind.Object)
                {
                    foreach (var key in SubjectKeys)
                    {
                        if (p.TryGetProperty(key, out var v) && v.ValueKind is JsonValueKind.String or JsonValueKind.Number
                            && v.ToString() is { Length: > 0 } s)
                        {
                            subject = s;
                            break;
                        }
                    }
                }
                var ts = root.TryGetProperty("ts", out var t) && t.ValueKind == JsonValueKind.String ? t.GetString() ?? "" : "";
                var ok = root.TryGetProperty("ok", out var o) && o.ValueKind == JsonValueKind.True;
                entries.Add(new ActionEntry(ts, verb.GetString() ?? "", subject, ok));
            }
            catch (JsonException)
            {
                // A torn last line while the daemon writes.
            }
        }
        return entries;
    }

    /// <summary>The newest <paramref name="max"/> entries, newest first.</summary>
    public static IReadOnlyList<ActionEntry> ReadNewestFirst(string filePath, int max)
    {
        try
        {
            if (!File.Exists(filePath))
                return [];
            var entries = Parse(File.ReadAllText(filePath));
            entries.Reverse();
            return entries.Take(max).ToList();
        }
        catch (IOException)
        {
            return [];
        }
    }

    /// <summary>"move window · firefox", "pause the music" style: the verb in words and what it acted on.</summary>
    public static string Describe(ActionEntry e) =>
        e.Verb.Replace('_', ' ') + (e.Subject.Length > 0 ? " · " + e.Subject : "") + (e.Ok ? "" : " (didn't work)");
}
