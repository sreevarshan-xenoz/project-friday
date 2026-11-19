namespace FridayTray.Core;

using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

/// <summary>What an agent run is waiting for.</summary>
public sealed record AgentPending(
    [property: JsonPropertyName("step")] string Step,
    [property: JsonPropertyName("why")] string Why);

/// <summary>One run recorded by the daemon in agents\&lt;id&gt;.json.</summary>
public sealed record AgentRun(
    [property: JsonPropertyName("id")] string Id,
    [property: JsonPropertyName("agent")] string Agent = "",
    [property: JsonPropertyName("folder")] string Folder = "",
    [property: JsonPropertyName("request")] string Request = "",
    [property: JsonPropertyName("task")] string? Task = null,
    [property: JsonPropertyName("status")] string Status = "",
    [property: JsonPropertyName("started")] long Started = 0,
    [property: JsonPropertyName("ended")] long? Ended = null,
    [property: JsonPropertyName("pending")] AgentPending? Pending = null,
    [property: JsonPropertyName("summary")] string? Summary = null,
    [property: JsonPropertyName("approved_by_friday")] int ApprovedByFriday = 0,
    [property: JsonPropertyName("approved_by_you")] int ApprovedByYou = 0,
    [property: JsonPropertyName("rejected")] int Rejected = 0)
{
    public bool IsQuestion => Pending?.Why == "has a question for you";
    public bool IsWaiting => Status == "waiting_for_you";
    public bool IsFinished => Status is "done" or "failed";
}

/// <summary>
/// Reader and answer writer for daemon agent runs and Claude Code hooks.
/// </summary>
public static class AgentRuns
{
    private static readonly Regex IdRegex = new(@"^r[0-9a-f]+$", RegexOptions.Compiled);

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    public static bool IsValidId(string id) => !string.IsNullOrEmpty(id) && IdRegex.IsMatch(id);

    /// <summary>Most runs read at once: the tray reads them every couple of seconds.</summary>
    public const int Recent = 20;

    /// <summary>
    /// Loads the most recently changed runs from <paramref name="dir"/>, newest
    /// first. Files that do not parse or whose id is not ^r[0-9a-f]+$ are skipped.
    /// </summary>
    public static IReadOnlyList<AgentRun> Load(string dir)
    {
        if (!Directory.Exists(dir))
            return [];

        var runs = new List<AgentRun>();
        var files = new DirectoryInfo(dir).EnumerateFiles("r*.json")
            .OrderByDescending(f => f.LastWriteTimeUtc)
            .Take(Recent)
            .Select(f => f.FullName);
        foreach (var file in files)
        {
            var id = Path.GetFileNameWithoutExtension(file);
            if (!IsValidId(id))
                continue;
            try
            {
                var text = File.ReadAllText(file);
                var run = JsonSerializer.Deserialize<AgentRun>(text, JsonOptions);
                if (run is not null && IsValidId(run.Id))
                    runs.Add(run);
            }
            catch (Exception e) when (e is JsonException or IOException or InvalidOperationException)
            {
                // skip corrupted or locked file
            }
        }
        runs.Sort((a, b) => b.Started.CompareTo(a.Started));
        return runs;
    }

    /// <summary>
    /// Writes the answer for a run waiting for the person (allow, deny, or reply:&lt;text&gt;).
    /// Validates the id and ensures the run is waiting_for_you.
    /// </summary>
    public static bool Answer(string dir, string id, string answer)
    {
        if (!IsValidId(id) || string.IsNullOrEmpty(answer))
            return false;

        var isAllow = answer == "allow";
        var isDeny = answer == "deny";
        var isReply = answer.StartsWith("reply:", StringComparison.Ordinal) && answer.Length > "reply:".Length;
        if (!isAllow && !isDeny && !isReply)
            return false;

        var runFile = Path.Combine(dir, $"{id}.json");
        if (!File.Exists(runFile))
            return false;

        try
        {
            var text = File.ReadAllText(runFile);
            var run = JsonSerializer.Deserialize<AgentRun>(text, JsonOptions);
            if (run is null || run.Status != "waiting_for_you")
                return false;
            if (isReply && !run.IsQuestion)
                return false;
        }
        catch (Exception e) when (e is JsonException or IOException or InvalidOperationException)
        {
            return false;
        }

        var answerFile = Path.Combine(dir, $"{id}.answer");
        var tmp = answerFile + ".tmp";
        try
        {
            File.WriteAllText(tmp, answer);
            File.Move(tmp, answerFile, overwrite: true);
            return true;
        }
        catch (IOException)
        {
            return false;
        }
    }

    /// <summary>
    /// Counts decisions FRIDAY approved in Claude Code sessions since <paramref name="unixTs"/>.
    /// </summary>
    public static int HooksApprovedSince(string dir, long unixTs)
    {
        var file = Path.Combine(dir, "hooks.jsonl");
        if (!File.Exists(file))
            return 0;

        var count = 0;
        try
        {
            foreach (var line in File.ReadLines(file))
            {
                var trimmed = line.Trim();
                if (trimmed.Length == 0)
                    continue;
                try
                {
                    using var doc = JsonDocument.Parse(trimmed);
                    var root = doc.RootElement;
                    if (!root.TryGetProperty("decision", out var dec) || dec.GetString() != "approved")
                        continue;
                    if (!root.TryGetProperty("ts", out var tsProp))
                        continue;

                    long ts;
                    if (tsProp.ValueKind == JsonValueKind.Number && tsProp.TryGetInt64(out var n))
                        ts = n;
                    else if (tsProp.ValueKind == JsonValueKind.String && long.TryParse(tsProp.GetString(), out var s))
                        ts = s;
                    else
                        continue;

                    if (ts >= unixTs)
                        count++;
                }
                catch (JsonException)
                {
                    // skip torn or invalid line
                }
            }
        }
        catch (IOException)
        {
            return 0;
        }
        return count;
    }
}
