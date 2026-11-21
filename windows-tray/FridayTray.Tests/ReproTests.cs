using System.Text.Json;
using FridayTray.Core;
using Xunit;

public class ReproTests
{
    [Fact]
    public void A_repro_keeps_how_it_was_asked_never_the_text()
    {
        var outcome = new AskOutcome(true, "Paris is the capital of France.", null, false,
            ServedBy: new ServedByInfo("kilo", "m1", 940), Masked: 1, Used: ["the window's text"]);
        var json = Repro.Json(AskKind.Screen, outcome, "what is the capital?".Length, "Wrong", "0.8.0",
            new DateTimeOffset(2026, 11, 22, 9, 30, 0, TimeSpan.Zero));
        Assert.DoesNotContain("Paris", json);
        Assert.DoesNotContain("capital", json);
        var r = JsonDocument.Parse(json).RootElement;
        Assert.Equal("screen", r.GetProperty("mode").GetString());
        Assert.Equal("kilo", r.GetProperty("served_by").GetProperty("provider").GetString());
        Assert.Equal(20, r.GetProperty("question_chars").GetInt32());
        Assert.Equal("Wrong", r.GetProperty("reason").GetString());
        Assert.Equal(JsonValueKind.Null, r.GetProperty("error").ValueKind);
    }

    [Theory]
    [InlineData("timed out after 180s waiting for the goal", "timeout")]
    [InlineData("provider opencode: 429 FreeUsageLimitError", "rate_limited")]
    [InlineData("Couldn't reach your AI provider", "unreachable")]
    [InlineData("something odd: my secret text", "other")]
    public void A_failure_is_kept_as_its_kind(string error, string kind)
    {
        var json = Repro.Json(AskKind.Plain, new AskOutcome(false, error, null, false), 5, "Wrong", "0.8.0", DateTimeOffset.Now);
        Assert.Equal(kind, JsonDocument.Parse(json).RootElement.GetProperty("error").GetString());
        Assert.DoesNotContain("secret", json);
    }

    [Fact]
    public void Save_writes_one_file_in_repros()
    {
        var dir = Path.Combine(Path.GetTempPath(), "friday-repro-" + Guid.NewGuid().ToString("N"));
        try
        {
            var paths = new Paths(dir, dir);
            var file = Repro.Save(paths, AskKind.Plain, new AskOutcome(true, "x", null, false), 3, "Too slow", "0.8.0");
            Assert.Equal(Path.Combine(paths.StateDir, "repros"), Path.GetDirectoryName(file));
            Assert.Contains("Too slow", File.ReadAllText(file));
        }
        finally { if (Directory.Exists(dir)) Directory.Delete(dir, true); }
    }
}
