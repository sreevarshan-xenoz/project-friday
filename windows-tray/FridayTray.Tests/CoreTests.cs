using FridayTray.Core;
using Xunit;

public class EnvFileTests
{
    [Fact]
    public void Parses_keys_comments_and_quotes()
    {
        var v = EnvFile.Parse("# c\nGEMINI_API_KEY=abc123secret\r\nFRIDAY_CONTEXT_PROVIDER=\"gemini\"\n\nbad line\n");
        Assert.Equal("abc123secret", v["GEMINI_API_KEY"]);
        Assert.Equal("gemini", v["FRIDAY_CONTEXT_PROVIDER"]);
        Assert.Equal(2, v.Count);
    }

    [Fact]
    public void Choosing_a_provider_replaces_keys_and_keeps_other_settings()
    {
        var before = "# old\nGROQ_API_KEY=gsk_old123456\nFRIDAY_CONTEXT_PROVIDER=groq\nFRIDAY_LISTEN=127.0.0.1:1\n";
        var after = EnvFile.Parse(EnvFile.WithProvider(before, Providers.Find("openrouter")!, "sk-or-12345678"));
        Assert.Equal("sk-or-12345678", after["OPENROUTER_API_KEY"]);
        Assert.Equal("127.0.0.1:1", after["FRIDAY_LISTEN"]);
        Assert.False(after.ContainsKey("GROQ_API_KEY"));
        // OpenRouter has no vision, so no context provider is set.
        Assert.False(after.ContainsKey("FRIDAY_CONTEXT_PROVIDER"));
    }

    [Fact]
    public void Keeping_other_keys_replaces_only_this_providers_key()
    {
        var before = "GEMINI_API_KEY=AIzaOld12345\nGROQ_API_KEY=gsk_old123456\nFRIDAY_CONTEXT_PROVIDER=gemini\nFRIDAY_LISTEN=127.0.0.1:1\n";
        var after = EnvFile.Parse(EnvFile.WithProvider(before, Providers.Find("groq")!, "gsk_new123456", keepOthers: true));
        Assert.Equal("AIzaOld12345", after["GEMINI_API_KEY"]);
        Assert.Equal("gsk_new123456", after["GROQ_API_KEY"]);
        Assert.Equal("groq", after["FRIDAY_CONTEXT_PROVIDER"]);
        Assert.Equal("127.0.0.1:1", after["FRIDAY_LISTEN"]);

        // A text-only provider leaves screen questions where they were.
        var text = EnvFile.Parse(EnvFile.WithProvider(before, Providers.Find("openrouter")!, "sk-or-12345678", keepOthers: true));
        Assert.Equal("gemini", text["FRIDAY_CONTEXT_PROVIDER"]);
        Assert.Equal(3, text.Keys.Count(k => k.EndsWith("_API_KEY")));
    }

    [Fact]
    public void Other_keys_lists_the_providers_besides_this_one()
    {
        var env = "GEMINI_API_KEY=a12345678\nGROQ_API_KEY=b12345678\nFRIDAY_LISTEN=x\n";
        Assert.True(EnvFile.HasOtherKeys(env, Providers.Find("opencode")!));
        Assert.False(EnvFile.HasOtherKeys("GROQ_API_KEY=b12345678\n", Providers.Find("groq")!));
        Assert.False(EnvFile.HasOtherKeys("", Providers.Find("groq")!));
    }

    [Fact]
    public void OpenCode_without_a_key_uses_the_free_models()
    {
        var after = EnvFile.Parse(EnvFile.WithProvider("GEMINI_API_KEY=x12345678\n", Providers.Find("opencode")!, ""));
        Assert.False(after.ContainsKey("OPENCODE_API_KEY"));
        Assert.False(after.ContainsKey("GEMINI_API_KEY"));
        Assert.Equal("opencode", after["FRIDAY_CONTEXT_PROVIDER"]);
    }

    [Theory]
    [InlineData("AIzaSyA-b_c.d:e", true)]
    [InlineData("short", false)]
    [InlineData("has space 12345", false)]
    public void Key_shape_matches_setup_ai(string key, bool ok) => Assert.Equal(ok, Providers.LooksLikeKey(key));
}

public class RestartPolicyTests
{
    [Fact]
    public void Backs_off_then_gives_up_after_five_crashes_in_a_minute()
    {
        var p = new RestartPolicy();
        var t = new DateTime(2026, 1, 1, 12, 0, 0);
        for (var i = 1; i <= 5; i++)
            Assert.Equal(TimeSpan.FromSeconds(i), p.OnCrash(t.AddSeconds(i)));
        Assert.Null(p.OnCrash(t.AddSeconds(10)));
    }

    [Fact]
    public void Crashes_spread_over_time_keep_restarting()
    {
        var p = new RestartPolicy();
        var t = new DateTime(2026, 1, 1, 12, 0, 0);
        for (var i = 0; i < 20; i++)
            Assert.Equal(TimeSpan.FromSeconds(1), p.OnCrash(t.AddMinutes(2 * i)));
    }
}

public class AnswerLogTests
{
    [Fact]
    public void History_is_newest_first_without_repeats_and_answers_are_kept()
    {
        var dir = Directory.CreateTempSubdirectory().FullName;
        var paths = new Paths(Path.Combine(dir, "local"), Path.Combine(dir, "roaming"));
        var log = new AnswerLog(paths);
        log.Remember("a"); log.Remember("b"); log.Remember("a");
        Assert.Equal(["a", "b"], log.History());

        var when = new DateTime(2026, 11, 5, 9, 30, 0);
        log.Save("why?", "some\ncontext", new AskOutcome(false, "plain message", "raw error", false), when);
        var text = File.ReadAllText(paths.LastAnswerFile);
        Assert.StartsWith("# why?\n\n_2026-11-05 09:30_\n\n> some context\n\nplain message\n", text);
        Assert.Contains("```\nraw error\n```", text);
        Assert.Contains("---", File.ReadAllText(paths.AnswersFile));
        Assert.Equal(Path.Combine(dir, "local", "friday", "state.json"), paths.StateFile);
    }
}

public class ProviderMappingTests
{
    [Theory]
    [InlineData("opencode-zen", "OpenCode Zen")]
    [InlineData("kilo", "Kilo")]
    [InlineData("claude-code", "Claude Code")]
    [InlineData("codex", "Codex")]
    [InlineData("antigravity", "Antigravity")]
    [InlineData("cline", "Cline")]
    [InlineData("llamacpp", "llamacpp")]
    [InlineData("custom-provider", "custom-provider")]
    public void Maps_provider_ids_to_readable_names(string id, string expected)
    {
        Assert.Equal(expected, Providers.DisplayName(id));
    }

    [Fact]
    public void Formats_served_by_attribution_line()
    {
        Assert.Equal("via OpenCode Zen · 2.3 s", Providers.FormatServedBy("opencode-zen", 2300));
        Assert.Equal("via Kilo · 0.5 s", Providers.FormatServedBy("kilo", 500));
        Assert.Equal("via llamacpp · 0.0 s", Providers.FormatServedBy("llamacpp", 45));
        Assert.Equal("via Codex · 2.0 s", Providers.FormatServedBy("codex", 2000));
    }

    [Fact]
    public void Asking_interpret_parses_served_by()
    {
        var output = """
            {
              "outcome": "completed",
              "result": "Hello world",
              "error": null,
              "served_by": {
                "provider": "opencode-zen",
                "model": "inclusionai/ling-3.0-flash:free",
                "ms": 2300
              }
            }
            """;
        var outcome = Asking.Interpret(AskKind.Plain, "say hello", 0, output);
        Assert.True(outcome.Ok);
        Assert.Equal("Hello world", outcome.Answer);
        Assert.NotNull(outcome.ServedBy);
        Assert.Equal("opencode-zen", outcome.ServedBy.Provider);
        Assert.Equal("inclusionai/ling-3.0-flash:free", outcome.ServedBy.Model);
        Assert.Equal(2300, outcome.ServedBy.Ms);
    }

    [Fact]
    public void Asking_interpret_says_what_the_answer_used()
    {
        var output = """
            {
              "outcome": "completed",
              "result": "Done.",
              "used": ["your timeline", "a lesson from a failure"]
            }
            """;
        var outcome = Asking.Interpret(AskKind.Selection, "explain this", 0, output);
        Assert.Equal(new[] { "your selection", "your timeline", "a lesson from a failure" }, outcome.Used);
        Assert.Equal("Used: your selection, your timeline, a lesson from a failure",
            Providers.FormatFooter(null, 0, outcome.Used));
        Assert.Equal("", Providers.FormatFooter(null, 0, Asking.Interpret(AskKind.Plain, "hi", 0, "{\"outcome\":\"completed\",\"result\":\"x\"}").Used));
    }

    [Fact]
    public void Asking_interpret_leaves_served_by_null_when_absent()
    {
        var output = """
            {
              "outcome": "completed",
              "result": "Hello world",
              "error": null
            }
            """;
        var outcome = Asking.Interpret(AskKind.Plain, "say hello", 0, output);
        Assert.True(outcome.Ok);
        Assert.Null(outcome.ServedBy);
    }

    [Fact]
    public void Asking_interpret_parses_masked()
    {
        var output = """
            {
              "outcome": "completed",
              "result": "Hello world",
              "error": null,
              "masked": 2
            }
            """;
        var outcome = Asking.Interpret(AskKind.Plain, "say hello", 0, output);
        Assert.True(outcome.Ok);
        Assert.Equal(2, outcome.Masked);
    }

    [Fact]
    public void Asking_interpret_leaves_masked_zero_when_absent()
    {
        var output = """
            {
              "outcome": "completed",
              "result": "Hello world",
              "error": null
            }
            """;
        var outcome = Asking.Interpret(AskKind.Plain, "say hello", 0, output);
        Assert.True(outcome.Ok);
        Assert.Equal(0, outcome.Masked);
    }

    [Fact]
    public void Formats_masked_footer_line()
    {
        Assert.Equal("1 secret(s) masked before sending", Providers.FormatMasked(1));
        Assert.Equal("3 secret(s) masked before sending", Providers.FormatMasked(3));

        var sb = new ServedByInfo("opencode-zen", "model", 2300);
        Assert.Equal("via OpenCode Zen · 2.3 s · 1 secret(s) masked before sending", Providers.FormatFooter(sb, 1));
        Assert.Equal("via OpenCode Zen · 2.3 s", Providers.FormatFooter(sb, 0));
        Assert.Equal("2 secret(s) masked before sending", Providers.FormatFooter(null, 2));
        Assert.Equal("", Providers.FormatFooter(null, 0));
    }
}
