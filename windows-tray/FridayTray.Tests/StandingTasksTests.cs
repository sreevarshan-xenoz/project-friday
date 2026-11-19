using System.Text.Json.Nodes;
using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class StandingTasksTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "friday-standing-" + Guid.NewGuid());
    private string File_ => Path.Combine(_dir, "standing.json");
    private string Queue => Path.Combine(_dir, "standing-due.jsonl");

    // The shape the daemon writes (serde, pretty-printed).
    private const string DaemonFile = """
        {
          "tasks": [
            { "id": "t1", "goal": "tell me the weather", "trigger": { "kind": "daily", "hour": 8, "minute": 5, "weekdays_only": true },
              "paused": false, "created": 1, "last_run": 2, "runs": 3, "armed": true },
            { "id": "t2", "goal": "check my mail", "trigger": { "kind": "every", "minutes": 120 },
              "paused": true, "created": 1, "last_run": null, "runs": 0, "armed": true },
            { "id": "t3", "goal": "tell me to plug in", "trigger": { "kind": "battery_below", "percent": 20 },
              "paused": false, "created": 1, "last_run": null, "runs": 0, "armed": false }
          ]
        }
        """;

    public StandingTasksTests()
    {
        Directory.CreateDirectory(_dir);
        File.WriteAllText(File_, DaemonFile);
    }

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Lists_the_daemons_tasks_in_its_own_words()
    {
        var tasks = StandingTasks.Load(File_);
        Assert.Equal(new[] { "t1", "t2", "t3" }, tasks.Select(t => t.Id));
        Assert.Equal("every weekday at 08:05", tasks[0].When);
        Assert.Equal("every 2 hours", tasks[1].When);
        Assert.Equal("when the battery is under 20%", tasks[2].When);
        Assert.True(tasks[1].Paused);
        Assert.Equal(3, tasks[0].Runs);
        Assert.Equal("every 45 minutes", StandingTasks.Describe(new JsonObject { ["kind"] = "every", ["minutes"] = 45 }));
        Assert.Equal("every hour", StandingTasks.Describe(new JsonObject { ["kind"] = "every", ["minutes"] = 60 }));
    }

    [Fact]
    public void A_missing_or_broken_file_is_no_tasks()
    {
        Assert.Empty(StandingTasks.Load(Path.Combine(_dir, "none.json")));
        File.WriteAllText(File_, "{ not json");
        Assert.Empty(StandingTasks.Load(File_));
        Assert.False(StandingTasks.Delete(File_, "t1"));
    }

    [Fact]
    public void Pausing_and_deleting_keep_what_the_daemon_wrote()
    {
        Assert.True(StandingTasks.SetPaused(File_, "t1", true));
        Assert.True(StandingTasks.Delete(File_, "t2"));
        Assert.False(StandingTasks.Delete(File_, "t9"));
        var root = JsonNode.Parse(File.ReadAllText(File_))!;
        var tasks = root["tasks"]!.AsArray();
        Assert.Equal(2, tasks.Count);
        Assert.True(tasks[0]!["paused"]!.GetValue<bool>());
        Assert.Equal(2, tasks[0]!["last_run"]!.GetValue<int>());
        Assert.False(tasks[1]!["armed"]!.GetValue<bool>());
        Assert.True(StandingTasks.SetPaused(File_, "t1", false));
        Assert.False(StandingTasks.Load(File_)[0].Paused);
    }

    [Fact]
    public void Due_goals_are_taken_once_in_order_and_bad_lines_skipped()
    {
        File.WriteAllLines(Queue, new[]
        {
            """{"goal":"tell me the weather","ts":1}""",
            "{ torn",
            """{"goal":"   ","ts":2}""",
            """{"goal":"check my mail","ts":3}""",
            "{\"goal\":\"" + new string('x', StandingTasks.MaxGoal + 1) + "\"}",
        });
        Assert.Equal(new[] { "tell me the weather", "check my mail" }, StandingTasks.TakeDue(Queue));
        Assert.False(File.Exists(Queue));
        Assert.Empty(StandingTasks.TakeDue(Queue));
    }
}
