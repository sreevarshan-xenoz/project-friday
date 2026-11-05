using FridayTray.Core;
using Xunit;

public class AskingTests
{
    [Fact]
    public void Request_adds_selected_text_as_context()
    {
        Assert.Equal("why?", Asking.BuildRequest("why?", null));
        Assert.Equal("why?", Asking.BuildRequest("why?", "  \n "));
        var r = Asking.BuildRequest("summarise", "line one\nline two");
        Assert.Equal("summarise\n\nUse this text as context:\n\"\"\"\nline one\nline two\n\"\"\"", r);
    }

    [Fact]
    public void Partial_answer_is_only_the_goal_in_flight()
    {
        const string live = """{"current":{"goal":"q","active":true,"partial":"The ans"}}""";
        Assert.Equal("The ans", Asking.PartialAnswer(live, "q"));
        Assert.Null(Asking.PartialAnswer(live, "another question"));
        Assert.Null(Asking.PartialAnswer("""{"current":{"goal":"q","active":false,"partial":"x"}}""", "q"));
        Assert.Null(Asking.PartialAnswer("""{"current":{"goal":"q","active":true,"partial":null}}""", "q"));
        Assert.Null(Asking.PartialAnswer("""{"current":null}""", "q"));
        Assert.Null(Asking.PartialAnswer("{ half a file", "q"));
    }

    [Fact]
    public void Partial_lines_are_the_answer_so_far()
    {
        Assert.Equal("The screen sh", Asking.PartialLine("""{"partial":"The screen sh"}"""));
        Assert.Null(Asking.PartialLine("""{"response":"done"}"""));
        Assert.Null(Asking.PartialLine("""{"partial": broken"""));
        Assert.Null(Asking.PartialLine("plain log line"));
    }

    [Fact]
    public void Screen_result_is_the_final_object_not_a_partial()
    {
        var output = "{\"partial\":\"A ter\"}\n{\"partial\":\"A terminal\"}\n{\n  \"response\": \"A terminal with an error.\"\n}\n";
        var o = Asking.Interpret(AskKind.Screen, "what is this", true, output);
        Assert.True(o.Ok);
        Assert.Equal("A terminal with an error.", o.Answer);
    }

    [Fact]
    public void Request_caps_context_length()
    {
        var r = Asking.BuildRequest("q", new string('x', 9000));
        Assert.Contains(new string('x', Asking.MaxContextChars) + "\n\"\"\"", r);
        Assert.DoesNotContain(new string('x', Asking.MaxContextChars + 1), r);
    }

    [Fact]
    public void Screen_asks_go_through_the_context_command()
    {
        Assert.Equal(["context", "screen", "--yes", "--ask", "what is this", "--stream"],
            Asking.CtlArguments(AskKind.Screen, "what is this", "ignored"));
        Assert.Equal(["goal", "--wait", "req"], Asking.CtlArguments(AskKind.Plain, "q", "req"));
    }

    [Theory]
    [InlineData("ghp_A1b2C3d4E5f6G7h8", true)]
    [InlineData("hunter2hunter2hunter2", true)]
    [InlineData("hello world this is text", false)]
    [InlineData("onlyletterslongword", false)]
    [InlineData("short1", false)]
    public void Clipboard_secrets_are_recognised(string text, bool secret) =>
        Assert.Equal(secret, Asking.LooksLikeSecret(text));

    [Fact]
    public void Json_objects_are_found_among_log_lines()
    {
        var output = "submitting goal\n{\"accepted\":true}\nwaiting...\n{\n \"outcome\": \"completed\",\n \"result\": \"Line one.\\nLine two.\"\n}\n";
        var objs = Asking.JsonObjects(output);
        Assert.Equal(2, objs.Count);
        Assert.Equal("completed", objs[1].GetProperty("outcome").GetString());
    }

    [Fact]
    public void Completed_goal_gives_the_full_answer()
    {
        var o = Asking.Interpret(AskKind.Plain, "q", true,
            "{\"accepted\":true}\n{\n \"outcome\": \"completed\",\n \"result\": \"Line one.\\nLast line.\"\n}");
        Assert.True(o.Ok);
        Assert.Equal("Line one.\nLast line.", o.Answer);
    }

    [Fact]
    public void Completed_goal_without_text_says_done() =>
        Assert.Equal("Done: tidy up", Asking.Interpret(AskKind.Plain, "tidy up", true, "{\"outcome\":\"completed\"}").Answer);

    [Fact]
    public void Free_tier_limit_is_explained()
    {
        var o = Asking.Interpret(AskKind.Plain, "q", false,
            "{\n \"outcome\": \"failed\",\n \"error\": \"provider opencode: 429 FreeUsageLimitError\"\n}");
        Assert.False(o.Ok);
        Assert.StartsWith("OpenCode Zen's free limit is used up", o.Answer);
        Assert.Contains("429", o.Details);
    }

    [Fact]
    public void Daemon_not_running_offers_to_start_it()
    {
        var o = Asking.Interpret(AskKind.Plain, "q", false, "no token found; daemon not running at C:\\x\\state.json");
        Assert.True(o.DaemonDown);
        Assert.StartsWith("FRIDAY isn't running", o.Answer);
        // Windows wording of a refused TCP connection.
        Assert.True(Asking.Interpret(AskKind.Plain, "q", false,
            "No connection could be made because the target machine actively refused it.").DaemonDown);
    }

    [Fact]
    public void Slow_goal_says_it_may_still_be_working() =>
        Assert.StartsWith("This is taking longer",
            Asking.Interpret(AskKind.Plain, "q", false, "{\"accepted\":true}\ntimed out after 180s waiting").Answer);

    [Fact]
    public void Other_failures_show_the_error() =>
        Assert.Equal("network unreachable",
            Asking.Interpret(AskKind.Plain, "q", false, "{\"outcome\":\"failed\",\"error\":\"network unreachable\"}").Answer);

    [Fact]
    public void Screen_answer_comes_from_the_response_field()
    {
        var o = Asking.Interpret(AskKind.Screen, "q", true, "{\n \"response\": \"The error says E0308.\\nFix the type.\"\n}");
        Assert.Equal("The error says E0308.\nFix the type.", o.Answer);
    }
}
