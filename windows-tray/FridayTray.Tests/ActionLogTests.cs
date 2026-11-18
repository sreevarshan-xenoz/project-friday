using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public sealed class ActionLogTests
{
    [Fact]
    public void Parse_ReadsVerbSubjectAndOutcome_AndSkipsATornLine()
    {
        var jsonl = """
            {"ts":"2026-11-19T10:00:00Z","verb":"focus_window","params":{"target":"notepad"},"risk":"reversible","ok":true,"goal":"x"}
            {"ts":"2026-11-19T10:01:00Z","verb":"close_window","params":{"target":"Paint"},"risk":"destructive","ok":false,"goal":"(confirmed)"}
            {"ts":"2026-11-19T10:02:0
            """;
        var entries = ActionLog.Parse(jsonl);
        Assert.Equal(2, entries.Count);
        Assert.Equal("focus window · notepad", ActionLog.Describe(entries[0]));
        Assert.Equal("close window · Paint (didn't work)", ActionLog.Describe(entries[1]));
    }

    [Fact]
    public void Parse_UsesNumbersAndNoParams()
    {
        var entries = ActionLog.Parse("""
            {"ts":"t","verb":"switch_desktop","params":{"desktop":2},"ok":true}
            {"ts":"t","verb":"now_playing","ok":true}
            """);
        Assert.Equal("switch desktop · 2", ActionLog.Describe(entries[0]));
        Assert.Equal("now playing", ActionLog.Describe(entries[1]));
    }

    [Fact]
    public void ReadNewestFirst_CapsAndHandlesAMissingFile()
    {
        var dir = Directory.CreateTempSubdirectory();
        var file = Path.Combine(dir.FullName, "actions.jsonl");
        Assert.Empty(ActionLog.ReadNewestFirst(file, 8));
        File.WriteAllLines(file, Enumerable.Range(1, 10).Select(i =>
            $$"""{"ts":"t{{i}}","verb":"media","params":{"command":"next"},"ok":true}"""));
        var got = ActionLog.ReadNewestFirst(file, 8);
        Assert.Equal(8, got.Count);
        Assert.Equal("t10", got[0].Ts);
        dir.Delete(true);
    }
}
