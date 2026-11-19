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
