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

    [Theory]
    [InlineData("search the web for rust releases", "rust releases")]
    [InlineData("Search the Web For rust releases", "rust releases")]
    [InlineData("search latest stable rust", "latest stable rust")]
    [InlineData("look up rust docs", "rust docs")]
    public void Web_questions_are_asked_with_a_search_and_lose_the_prefix(string typed, string asked)
    {
        Assert.Equal((asked, true), Asking.WebQuestion(typed));
    }

    [Theory]
    [InlineData("what is rust")]
    [InlineData("searching for meaning")]
    [InlineData("research this")]
    [InlineData("search")]
    [InlineData("look up")]
    public void Other_questions_never_search_the_web(string typed)
    {
        Assert.Equal((typed, false), Asking.WebQuestion(typed));
    }

    [Fact]
    public void Ctl_arguments_carry_the_web_flag_and_the_retry_options()
    {
        Assert.Equal(["goal", "--wait", "q"], Asking.CtlArguments(AskKind.Plain, "q", "q"));
        Assert.Equal(["goal", "--wait", "--web", "q"],
            Asking.CtlArguments(AskKind.Plain, "q", "q", null, new AskOptions(Web: true)));
        Assert.Equal(["goal", "--wait", "--thorough", "--avoid", "opencode/big-pickle", "q"],
            Asking.CtlArguments(AskKind.Plain, "q", "q", null, new AskOptions(Thorough: true, Avoid: "opencode/big-pickle")));
        // No model to avoid (or a malformed one): still thorough, nothing avoided.
        Assert.Equal(["goal", "--wait", "--thorough", "q"],
            Asking.CtlArguments(AskKind.Plain, "q", "q", null, new AskOptions(Thorough: true, Avoid: "nonsense")));
        Assert.Equal(["goal", "--wait", "--confirm", "git status", "--web", "q"],
            Asking.CtlArguments(AskKind.Plain, "q", "q", "git status", new AskOptions(Web: true)));
        // A screen ask goes through the screen path, whatever the options.
        Assert.Equal(["context", "screen", "--yes", "--ask", "q", "--stream"],
            Asking.CtlArguments(AskKind.Screen, "q", "q", null, new AskOptions(Thorough: true)));
    }

    [Fact]
    public void A_follow_up_carries_the_answer_it_follows_capped()
    {
        var r = Asking.BuildFollowUp("give an example", "A monad wraps a value.");
        Assert.Equal("give an example\n\n[follow-up]\nThis follows up on my previous question and your answer:\n\"\"\"\nA monad wraps a value.\n\"\"\"", r);
        Assert.DoesNotContain(new string('x', Asking.MaxFollowUpContextChars + 1), Asking.BuildFollowUp("q", new string('x', 9000)));
    }

    [Fact]
    public void The_model_to_avoid_comes_from_who_answered()
    {
        Assert.Equal("kilo/some/model", Asking.AvoidFor(new ServedByInfo("kilo", "some/model", 900)));
        Assert.Null(Asking.AvoidFor(null));
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
    [InlineData("https://github.com/x/y/issues/1234", false)]
    [InlineData(@"C:\Users\me\file2.txt", false)]
    [InlineData("/home/me/notes/file2.txt", false)]
    [InlineData("report2026.pdf", false)]
    [InlineData("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcDEF123456ghiJKL789", true)]
    [InlineData("https://api.x.com/v1?key=sk_live_abcd1234efgh5678ijkl", true)]
    [InlineData("https://x.com/cb?token=a1b2c3d4e5f6g7h8", true)]
    [InlineData("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", true)]
    [InlineData("hunter2hunter2.", true)]
    [InlineData("pass/12345678abc", true)]
    [InlineData("com.example.app2.v3", false)]
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
        // friday-ctl's own message when the daemon's port refuses (Windows wording).
        Assert.True(Asking.Interpret(AskKind.Plain, "q", false,
            "cannot reach daemon at ws://127.0.0.1:18432: IO error: No connection could be made because the target machine actively refused it.").DaemonDown);
    }

    [Fact]
    public void A_model_server_refusing_is_not_friday_down()
    {
        var o = Asking.Interpret(AskKind.Plain, "q", false,
            "{\"outcome\":\"failed\",\"error\":\"Couldn't reach your AI provider. Check your internet connection (or that your local model server is running).\"}\n"
            + "provider llamacpp: tcp connect error: No connection could be made because the target machine actively refused it.");
        Assert.False(o.DaemonDown);
        Assert.StartsWith("Couldn't reach your AI provider", o.Answer);
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

    [Fact]
    public void Output_parsing_with_needs_confirmation()
    {
        const string terminalOutput = """
            submitting goal
            {"accepted":true}
            waiting...
            {
              "outcome": "failed",
              "error": "Confirm to run: git status",
              "needs_confirmation": {
                "tool": "terminal",
                "action": "git status"
              }
            }
            """;
        var conf = Asking.ParseConfirmation(terminalOutput);
        Assert.NotNull(conf);
        Assert.Equal("terminal", conf.Tool);
        Assert.Equal("git status", conf.Action);

        const string openOutput = """
            {
              "outcome": "failed",
              "error": "Confirm to run: notepad.exe",
              "needs_confirmation": {
                "tool": "windows.open",
                "action": "notepad.exe"
              }
            }
            """;
        var openConf = Asking.ParseConfirmation(openOutput);
        Assert.NotNull(openConf);
        Assert.Equal("windows.open", openConf.Tool);
        Assert.Equal("notepad.exe", openConf.Action);

        const string writeOutput = """
            {
              "outcome": "failed",
              "error": "Confirm to run: write C:\\notes.txt",
              "needs_confirmation": {
                "tool": "filesystem.write",
                "action": "write C:\\notes.txt"
              }
            }
            """;
        var writeConf = Asking.ParseConfirmation(writeOutput);
        Assert.NotNull(writeConf);
        Assert.Equal("filesystem.write", writeConf.Tool);
        Assert.Equal("write C:\\notes.txt", writeConf.Action);
    }

    [Fact]
    public void Output_parsing_without_needs_confirmation()
    {
        Assert.Null(Asking.ParseConfirmation("{\"outcome\":\"completed\",\"result\":\"All good\"}"));
        Assert.Null(Asking.ParseConfirmation("{\"outcome\":\"failed\",\"error\":\"Some other failure\"}"));
        Assert.Null(Asking.ParseConfirmation("plain log line without json"));
        Assert.Null(Asking.ParseConfirmation(""));
    }

    [Fact]
    public void Exit_code_3_maps_to_needs_confirmation()
    {
        const string output = """
            {
              "outcome": "failed",
              "error": "Confirm to run: git push --force",
              "needs_confirmation": {
                "tool": "terminal",
                "action": "git push --force"
              }
            }
            """;

        // Exit code 3 maps to confirmation
        var conf3 = Asking.ParseConfirmation(3, output);
        Assert.NotNull(conf3);
        Assert.Equal("terminal", conf3.Tool);
        Assert.Equal("git push --force", conf3.Action);

        var outcome3 = Asking.Interpret(AskKind.Plain, "push changes", 3, output);
        Assert.False(outcome3.Ok);
        Assert.NotNull(outcome3.Confirmation);
        Assert.Equal("terminal", outcome3.Confirmation.Tool);
        Assert.Equal("git push --force", outcome3.Confirmation.Action);
        Assert.Equal("FRIDAY wants to run: git push --force", outcome3.Answer);

        // Exit code 1 does not map to confirmation
        var conf1 = Asking.ParseConfirmation(1, output);
        Assert.Null(conf1);

        var outcome1 = Asking.Interpret(AskKind.Plain, "push changes", 1, output);
        Assert.False(outcome1.Ok);
        Assert.Null(outcome1.Confirmation);

        // Exit code 0 does not map to confirmation
        var conf0 = Asking.ParseConfirmation(0, output);
        Assert.Null(conf0);

        var outcome0 = Asking.Interpret(AskKind.Plain, "push changes", 0, "{\"outcome\":\"completed\",\"result\":\"Pushed\"}");
        Assert.True(outcome0.Ok);
        Assert.Null(outcome0.Confirmation);
    }

    [Fact]
    public void CtlArguments_with_confirm_adds_flag()
    {
        var args = Asking.CtlArguments(AskKind.Plain, "list files", "list files", "dir");
        Assert.Equal(["goal", "--wait", "--confirm", "dir", "list files"], args);

        var argsNull = Asking.CtlArguments(AskKind.Plain, "list files", "list files", null);
        Assert.Equal(["goal", "--wait", "list files"], argsNull);
    }

    [Fact]
    public void CtlArguments_with_confirm_action_containing_quotes_and_spaces_stays_one_argument()
    {
        const string action = "git commit -m \"fix(tray): add confirmation\"";
        var args = Asking.CtlArguments(AskKind.Plain, "commit changes", "commit my changes", action);

        Assert.Equal(5, args.Length);
        Assert.Equal("goal", args[0]);
        Assert.Equal("--wait", args[1]);
        Assert.Equal("--confirm", args[2]);
        Assert.Equal(action, args[3]);
        Assert.Equal("commit my changes", args[4]);

        const string actionWithComplexQuotes = "cmd /c \"echo 'hello world' && exit 1\"";
        var args2 = Asking.CtlArguments(AskKind.Plain, "run script", "run script", actionWithComplexQuotes);
        Assert.Equal(5, args2.Length);
        Assert.Equal(actionWithComplexQuotes, args2[3]);
    }
}
