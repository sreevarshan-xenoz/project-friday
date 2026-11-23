using System.Text.Json;
using System.Text.Json.Nodes;

namespace FridayTray.Core;

/// <summary>One standing task as the Tasks page shows it.</summary>
public sealed record StandingTask(string Id, string Goal, string When, bool Paused, int Runs);

/// <summary>
/// Standing tasks ("every morning at 8, tell me the weather"): the daemon keeps
/// them in standing.json and decides when one is due; on Windows it leaves the
/// due goal in standing-due.jsonl and the tray asks it like the person's own
/// ask. This reads the list, pauses, resumes and deletes (keeping every field
/// the daemon wrote), and takes the due goals.
/// </summary>
public static class StandingTasks
{
    /// <summary>Ready-made standing tasks (the daemon's `standing::RECIPES`):
    /// `friday-ctl tasks recipe &lt;id&gt;` adds one.</summary>
    public static readonly (string Id, string What)[] Recipes =
    [
        ("morning-brief", "Weekday mornings at 9: what you missed"),
        ("weather", "Every morning at 7:30: today's weather"),
        ("plug-in", "When the battery is under 20%: a reminder to plug in"),
        ("health-check", "Once a week: how the computer is doing"),
        ("tech-news", "Every evening at 6: the day's top tech news"),
    ];

    /// <summary>Longest goal the daemon accepts; anything longer in the queue is not ours.</summary>
    public const int MaxGoal = 300;

    public static IReadOnlyList<StandingTask> Load(string path)
    {
        var tasks = new List<StandingTask>();
        if (Read(path)?["tasks"] is not JsonArray all)
            return tasks;
        foreach (var t in all.OfType<JsonObject>())
        {
            var id = t["id"]?.GetValue<string>();
            var goal = t["goal"]?.GetValue<string>();
            if (id is null || goal is null)
                continue;
            tasks.Add(new StandingTask(id, goal, Describe(t["trigger"] as JsonObject),
                t["paused"]?.GetValue<bool>() ?? false, t["runs"]?.GetValue<int>() ?? 0));
        }
        return tasks;
    }

    /// <summary>The same phrase the daemon uses: "every day at 08:00".</summary>
    public static string Describe(JsonObject? trigger)
    {
        int Int(string key) => trigger?[key]?.GetValue<int>() ?? 0;
        switch (trigger?["kind"]?.GetValue<string>())
        {
            case "daily":
                var day = trigger["weekdays_only"]?.GetValue<bool>() == true ? "weekday" : "day";
                return $"every {day} at {Int("hour"):00}:{Int("minute"):00}";
            case "every":
                var m = Int("minutes");
                if (m % 60 == 0)
                    return m == 60 ? "every hour" : $"every {m / 60} hours";
                return $"every {m} minutes";
            case "battery_below":
                return $"when the battery is under {Int("percent")}%";
            case "download_done":
                return "when a download finishes";
            case "file_changed":
                return $"when {trigger["path"]?.GetValue<string>()} changes";
            case "app_opens":
                return $"when {trigger["app"]?.GetValue<string>()} opens";
            case "once":
                var at = DateTimeOffset.FromUnixTimeSeconds(trigger["at"]?.GetValue<long>() ?? 0).ToLocalTime();
                return $"once at {at:HH:mm} on {at:d MMM}";
            default:
                return "on a schedule";
        }
    }

    public static bool SetPaused(string path, string id, bool paused) =>
        Edit(path, tasks =>
        {
            var t = tasks.OfType<JsonObject>().FirstOrDefault(t => t["id"]?.GetValue<string>() == id);
            if (t is null)
                return false;
            t["paused"] = paused;
            return true;
        });

    public static bool Delete(string path, string id) =>
        Edit(path, tasks =>
        {
            var t = tasks.OfType<JsonObject>().FirstOrDefault(t => t["id"]?.GetValue<string>() == id);
            return t is not null && tasks.Remove(t);
        });

    /// <summary>
    /// The goals the daemon queued, oldest first, removed from the queue. The
    /// file is moved aside before reading, so a goal the daemon queues meanwhile
    /// lands in a new file and waits for the next call.
    /// </summary>
    public static IReadOnlyList<string> TakeDue(string queuePath)
    {
        var goals = new List<string>();
        if (!File.Exists(queuePath))
            return goals;
        var taking = queuePath + ".taking";
        try
        {
            File.Move(queuePath, taking, overwrite: true);
        }
        catch (IOException)
        {
            return goals; // the daemon is writing it; next time
        }
        foreach (var line in File.ReadLines(taking))
        {
            try
            {
                var goal = JsonNode.Parse(line)?["goal"]?.GetValue<string>()?.Trim();
                if (!string.IsNullOrEmpty(goal) && goal.Length <= MaxGoal)
                    goals.Add(goal);
            }
            catch (Exception e) when (e is JsonException or InvalidOperationException)
            {
                // a torn or foreign line: skip it
            }
        }
        File.Delete(taking);
        return goals;
    }

    private static JsonNode? Read(string path)
    {
        try
        {
            return File.Exists(path) ? JsonNode.Parse(File.ReadAllText(path)) : null;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>Quiet hours as the daemon reads them (`quiet: [22, 7]`), or null.</summary>
    public static (int Start, int End)? Quiet(string path) =>
        Read(path)?["quiet"] is JsonArray { Count: 2 } q ? (q[0]!.GetValue<int>(), q[1]!.GetValue<int>()) : null;

    /// <summary>Recurring tasks wait out 22:00–07:00 (on), or never (off).</summary>
    public static bool SetQuiet(string path, bool on)
    {
        var root = Read(path) as JsonObject ?? new JsonObject { ["tasks"] = new JsonArray() };
        if (on)
            root["quiet"] = new JsonArray(22, 7);
        else
            root.Remove("quiet");
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, root.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        File.Move(tmp, path, overwrite: true);
        return true;
    }

    private static bool Edit(string path, Func<JsonArray, bool> change)
    {
        if (Read(path) is not JsonObject root || root["tasks"] is not JsonArray tasks || !change(tasks))
            return false;
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, root.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        File.Move(tmp, path, overwrite: true);
        return true;
    }
}
