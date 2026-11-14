using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public sealed class EgressLogTests
{
    [Fact]
    public void Parse_EmptyContent_ReturnsEmpty()
    {
        Assert.Empty(EgressLog.Parse(""));
        Assert.Empty(EgressLog.Parse("   \n\n  "));
    }

    [Fact]
    public void Parse_ValidJsonl_ExtractsEntries()
    {
        var jsonl = """
            {"ts":"2026-11-14T18:00:00Z","goal_id":"goal-1","provider":"opencode","model":"inclusionai/ling-3.0-flash:free","kinds":["text"],"text_chars":1240,"image_bytes":0,"masked":0,"outcome":"ok"}
            {"ts":"2026-11-14T18:05:00Z","goal_id":"goal-2","provider":"kilo","model":"kilo-model","kinds":["text","image"],"text_chars":310,"image_bytes":186368,"masked":1,"outcome":"ok","local":true}
            """;
        var entries = EgressLog.Parse(jsonl);
        Assert.Equal(2, entries.Count);

        var first = entries[0];
        Assert.Equal("2026-11-14T18:00:00Z", first.Ts);
        Assert.Equal("goal-1", first.GoalId);
        Assert.Equal("opencode", first.Provider);
        Assert.Equal("inclusionai/ling-3.0-flash:free", first.Model);
        Assert.Equal(["text"], first.Kinds);
        Assert.Equal(1240, first.TextChars);
        Assert.Equal(0, first.ImageBytes);
        Assert.Equal(0, first.Masked);
        Assert.Equal("ok", first.Outcome);
        Assert.Null(first.Local);

        var second = entries[1];
        Assert.Equal("kilo", second.Provider);
        Assert.Equal(310, second.TextChars);
        Assert.Equal(186368, second.ImageBytes);
        Assert.Equal(1, second.Masked);
        Assert.True(second.Local);
    }

    [Fact]
    public void Parse_SkipsCorruptedLines()
    {
        var jsonl = """
            not json at all
            {"ts":"2026-11-14T18:00:00Z","provider":"opencode","model":"m1","kinds":["text"],"text_chars":100,"image_bytes":0,"outcome":"ok"}
            {broken:json}
            """;
        var entries = EgressLog.Parse(jsonl);
        Assert.Single(entries);
        Assert.Equal("opencode", entries[0].Provider);
    }

    [Fact]
    public void FormatWhat_TextOnly_FormatsWithCommas()
    {
        Assert.Equal("text 1,240 chars", EgressLog.FormatWhat(1240, 0));
        Assert.Equal("text 42 chars", EgressLog.FormatWhat(42, 0));
    }

    [Fact]
    public void FormatWhat_WithScreenshot_FormatsKb()
    {
        Assert.Equal("text 310 chars + screenshot 182 KB", EgressLog.FormatWhat(310, 186368));
        Assert.Equal("text 10 chars + screenshot 500 B", EgressLog.FormatWhat(10, 500));
    }

    [Fact]
    public void FormatProvider_UsesDisplayNameMapping()
    {
        Assert.Equal("OpenCode Zen", EgressLog.FormatProvider("opencode"));
        Assert.Equal("OpenCode Zen", EgressLog.FormatProvider("opencode-zen"));
        Assert.Equal("Kilo", EgressLog.FormatProvider("kilo"));
        Assert.Equal("Claude Code", EgressLog.FormatProvider("claude-code"));
        Assert.Equal("Codex", EgressLog.FormatProvider("codex"));
        Assert.Equal("Antigravity", EgressLog.FormatProvider("antigravity"));
        Assert.Equal("Cline", EgressLog.FormatProvider("cline"));
        Assert.Equal("llamacpp", EgressLog.FormatProvider("llamacpp"));
        Assert.Equal("ollama", EgressLog.FormatProvider("ollama"));
        Assert.Equal("custom-prov", EgressLog.FormatProvider("custom-prov"));
    }

    [Fact]
    public void FormatSummary_Empty_ReturnsNothingSent()
    {
        Assert.Equal("Nothing has been sent yet.", EgressLog.FormatSummary([], DateTime.UtcNow));
    }

    [Fact]
    public void FormatSummary_MatchesExpectedShape()
    {
        var now = new DateTime(2026, 11, 14, 18, 0, 0, DateTimeKind.Utc);
        var entries = new List<EgressEntry>();

        // 30 OpenCode Zen
        for (int i = 0; i < 30; i++)
        {
            entries.Add(new EgressEntry
            {
                Ts = now.AddHours(-i).ToString("o"),
                Provider = "opencode",
                Model = "m1",
                Kinds = ["text"],
                TextChars = 100,
                ImageBytes = 0,
                Outcome = "ok",
            });
        }
        // 12 Kilo (3 of them with a screenshot)
        for (int i = 0; i < 12; i++)
        {
            entries.Add(new EgressEntry
            {
                Ts = now.AddHours(-i).ToString("o"),
                Provider = "kilo",
                Model = "m2",
                Kinds = i < 3 ? ["text", "image"] : ["text"],
                TextChars = 200,
                ImageBytes = i < 3 ? 186368 : 0,
                Outcome = "ok",
            });
        }
        // 5 masked secrets across entries
        entries[0].Masked = 2;
        entries[1].Masked = 3;

        var summary = EgressLog.FormatSummary(entries, now);
        Assert.Equal("42 requests in the last 7 days: 30 OpenCode Zen, 12 Kilo; 3 with a screenshot; 5 secrets masked", summary);
    }

    [Fact]
    public void ReadNewestFirst_OrdersNewestFirst()
    {
        var tempFile = Path.GetTempFileName();
        try
        {
            File.WriteAllLines(tempFile,
            [
                """{"ts":"2026-11-14T10:00:00Z","provider":"p1","model":"m1","kinds":["text"],"text_chars":10,"image_bytes":0,"outcome":"ok"}""",
                """{"ts":"2026-11-14T11:00:00Z","provider":"p2","model":"m2","kinds":["text"],"text_chars":20,"image_bytes":0,"outcome":"ok"}"""
            ]);

            var entries = EgressLog.ReadNewestFirst(tempFile);
            Assert.Equal(2, entries.Count);
            Assert.Equal("p2", entries[0].Provider);
            Assert.Equal("p1", entries[1].Provider);
        }
        finally
        {
            File.Delete(tempFile);
        }
    }
}
