using System.Text.Json.Nodes;
using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class AgentRunsTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "friday-agents-" + Guid.NewGuid());

    public AgentRunsTests()
    {
        Directory.CreateDirectory(_dir);
    }

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Loads_runs_newest_first_and_skips_bad_or_unparseable_files()
    {
        // Valid run 1 (older)
        File.WriteAllText(Path.Combine(_dir, "r1a2b3.json"), """
        {
          "id": "r1a2b3",
          "agent": "opencode",
          "folder": "C:\\code\\site",
          "request": "add a README",
          "task": "write readme",
          "status": "running",
          "started": 1000,
          "ended": null,
          "pending": null,
          "summary": null,
          "approved_by_friday": 1,
          "approved_by_you": 0,
          "rejected": 0
        }
        """);

        // Valid run 2 (newer, waiting for you)
        File.WriteAllText(Path.Combine(_dir, "r4f5.json"), """
        {
          "id": "r4f5",
          "agent": "agy",
          "folder": "C:\\code\\api",
          "request": "fix tests",
          "task": "run cargo test",
          "status": "waiting_for_you",
          "started": 2000,
          "ended": null,
          "pending": { "step": "git push", "why": "runs `git push`" },
          "summary": "all tests pass",
          "approved_by_friday": 2,
          "approved_by_you": 1,
          "rejected": 0
        }
        """);

        // Bad filename (does not match ^r[0-9a-f]+$)
        File.WriteAllText(Path.Combine(_dir, "other.json"), """{"id":"other","status":"done"}""");
        File.WriteAllText(Path.Combine(_dir, "rGHI.json"), """{"id":"rGHI","status":"done"}""");

        // Torn json file
        File.WriteAllText(Path.Combine(_dir, "r999.json"), "{ invalid json");

        var runs = AgentRuns.Load(_dir);
        Assert.Equal(2, runs.Count);
        Assert.Equal("r4f5", runs[0].Id);
        Assert.Equal("r1a2b3", runs[1].Id);
        Assert.Equal("waiting_for_you", runs[0].Status);
        Assert.True(runs[0].IsWaiting);
        Assert.False(runs[0].IsQuestion);
        Assert.Equal("git push", runs[0].Pending?.Step);
    }

    [Fact]
    public void Loads_the_worktree_when_the_run_has_one_and_null_when_not()
    {
        File.WriteAllText(Path.Combine(_dir, "r20.json"),
            """{ "id": "r20", "status": "done", "started": 1, "worktree": "C:\\wt\\r20" }""");
        File.WriteAllText(Path.Combine(_dir, "r21.json"), """{ "id": "r21", "status": "done", "started": 2 }""");

        var runs = AgentRuns.Load(_dir).ToDictionary(r => r.Id);
        Assert.Equal(@"C:\wt\r20", runs["r20"].Worktree);
        Assert.Null(runs["r21"].Worktree);
    }

    [Fact]
    public void Identifies_question_runs()
    {
        File.WriteAllText(Path.Combine(_dir, "r10.json"), """
        {
          "id": "r10",
          "agent": "claude",
          "folder": "C:\\code\\app",
          "request": "build feature",
          "status": "waiting_for_you",
          "started": 1500,
          "pending": { "step": "Which port should we use?", "why": "has a question for you" }
        }
        """);

        var runs = AgentRuns.Load(_dir);
        Assert.Single(runs);
        Assert.True(runs[0].IsQuestion);
        Assert.Equal("Which port should we use?", runs[0].Pending?.Step);
    }

    [Fact]
    public void Answers_waiting_run_and_validates_id_and_status()
    {
        var runFile = Path.Combine(_dir, "r1.json");
        File.WriteAllText(runFile, """
        {
          "id": "r1",
          "agent": "opencode",
          "folder": "C:\\code\\test",
          "request": "test",
          "status": "waiting_for_you",
          "started": 100,
          "pending": { "step": "rm -rf build", "why": "deletes build folder" }
        }
        """);

        // Invalid IDs
        Assert.False(AgentRuns.Answer(_dir, "invalid-id", "allow"));
        Assert.False(AgentRuns.Answer(_dir, "rXYZ", "allow"));
        Assert.False(AgentRuns.Answer(_dir, "", "allow"));

        // Invalid answer strings
        Assert.False(AgentRuns.Answer(_dir, "r1", "yes"));
        Assert.False(AgentRuns.Answer(_dir, "r1", "ok"));
        Assert.False(AgentRuns.Answer(_dir, "r1", "reply:"));
        Assert.False(AgentRuns.Answer(_dir, "r1", "reply:hello")); // not a question run

        // Successful allow
        Assert.True(AgentRuns.Answer(_dir, "r1", "allow"));
        var answerFile = Path.Combine(_dir, "r1.answer");
        Assert.True(File.Exists(answerFile));
        Assert.Equal("allow", File.ReadAllText(answerFile));

        // Successful deny
        Assert.True(AgentRuns.Answer(_dir, "r1", "deny"));
        Assert.Equal("deny", File.ReadAllText(answerFile));
    }

    [Fact]
    public void Answers_question_run_with_reply()
    {
        var runFile = Path.Combine(_dir, "r2.json");
        File.WriteAllText(runFile, """
        {
          "id": "r2",
          "agent": "opencode",
          "folder": "C:\\code\\test",
          "request": "test",
          "status": "waiting_for_you",
          "started": 100,
          "pending": { "step": "Continue?", "why": "has a question for you" }
        }
        """);

        Assert.True(AgentRuns.Answer(_dir, "r2", "reply:yes, use port 3000"));
        var answerFile = Path.Combine(_dir, "r2.answer");
        Assert.Equal("reply:yes, use port 3000", File.ReadAllText(answerFile));
    }

    [Fact]
    public void Cannot_answer_run_not_waiting_for_you()
    {
        File.WriteAllText(Path.Combine(_dir, "r3.json"), """
        {
          "id": "r3",
          "agent": "opencode",
          "folder": "C:\\code\\test",
          "request": "test",
          "status": "running",
          "started": 100
        }
        """);
        Assert.False(AgentRuns.Answer(_dir, "r3", "allow"));

        File.WriteAllText(Path.Combine(_dir, "r4.json"), """
        {
          "id": "r4",
          "agent": "opencode",
          "folder": "C:\\code\\test",
          "request": "test",
          "status": "done",
          "started": 100
        }
        """);
        Assert.False(AgentRuns.Answer(_dir, "r4", "allow"));
    }

    [Fact]
    public void Counts_hooks_approved_since_timestamp()
    {
        var hooksFile = Path.Combine(_dir, "hooks.jsonl");
        File.WriteAllLines(hooksFile, [
            """{"agent":"claude","folder":"C:\\code\\a","step":"npm install","decision":"approved","why":"routine","ts":1000}""",
            """{"agent":"claude","folder":"C:\\code\\b","step":"git push","decision":"left to you","why":"risky","ts":1050}""",
            """{"agent":"claude","folder":"C:\\code\\c","step":"cargo build","decision":"approved","why":"routine","ts":1100}""",
            """{"agent":"claude","folder":"C:\\code\\d","step":"cargo test","decision":"approved","why":"routine","ts":1200}""",
            "{ torn line"
        ]);

        Assert.Equal(3, AgentRuns.HooksApprovedSince(_dir, 1000));
        Assert.Equal(2, AgentRuns.HooksApprovedSince(_dir, 1051));
        Assert.Equal(1, AgentRuns.HooksApprovedSince(_dir, 1150));
        Assert.Equal(0, AgentRuns.HooksApprovedSince(_dir, 1300));

        // Missing file is 0
        Assert.Equal(0, AgentRuns.HooksApprovedSince(Path.Combine(_dir, "missing"), 1000));
    }
}
