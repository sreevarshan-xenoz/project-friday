using System.Runtime.InteropServices;
using FridayTray.Core;
using Xunit;

public class AgentClisTests
{
    [Fact]
    public void All_clis_have_expected_identifiers_labels_hints_and_logins()
    {
        Assert.Equal(5, AgentClis.All.Count);

        var claude = AgentClis.Find("claude");
        Assert.NotNull(claude);
        Assert.Equal("Claude Code (your Anthropic account)", claude.Label);
        Assert.Equal("claude", claude.BinaryName);
        Assert.Equal("irm https://claude.ai/install.ps1 | iex", claude.InstallHint);
        Assert.Equal("claude auth login", claude.LoginCommand);

        var codex = AgentClis.Find("codex");
        Assert.NotNull(codex);
        Assert.Equal("Codex (your OpenAI / ChatGPT account)", codex.Label);
        Assert.Equal("codex", codex.BinaryName);
        Assert.Contains("codex.exe", codex.InstallHint);
        Assert.Equal("codex login", codex.LoginCommand);

        var agy = AgentClis.Find("agy");
        Assert.NotNull(agy);
        Assert.Equal("Antigravity (your Google account)", agy.Label);
        Assert.Equal("agy", agy.BinaryName);
        Assert.Equal("install the Antigravity CLI (agy)", agy.InstallHint);
        Assert.Equal("agy", agy.LoginCommand);

        var cline = AgentClis.Find("cline");
        Assert.NotNull(cline);
        Assert.Equal("Cline (your Cline account, incl. its free models)", cline.Label);
        Assert.Equal("cline", cline.BinaryName);
        Assert.Contains("can't use on Windows yet", cline.InstallHint);
        Assert.Equal("cline auth", cline.LoginCommand);
    }

    [Fact]
    public void Headless_check_argv_matches_specification()
    {
        const string q = "what is 2+2?";

        var claude = AgentClis.Find("claude")!;
        Assert.Equal(["-p", q, "--output-format", "text"], claude.CheckArgs(q));

        var codex = AgentClis.Find("codex")!;
        Assert.Equal(["exec", "--skip-git-repo-check", "--sandbox", "read-only", q], codex.CheckArgs(q));

        var agy = AgentClis.Find("agy")!;
        Assert.Equal(["-p", q, "--mode", "plan", "--effort", "low"], agy.CheckArgs(q));

        var cline = AgentClis.Find("cline")!;
        Assert.Equal(["-p", "--auto-approve", "false", q], cline.CheckArgs(q));

        var hermes = AgentClis.Find("hermes")!;
        Assert.Equal(["--ignore-rules", "-z", q, "-t", "context_engine"], hermes.CheckArgs(q));
    }

    [Fact]
    public void Default_check_prompt_is_used_when_omitted()
    {
        var claude = AgentClis.Find("claude")!;
        Assert.Contains(AgentClis.DefaultCheckPrompt, claude.CheckArgs());
        Assert.Equal("Reply with the single word: ready", AgentClis.DefaultCheckPrompt);
    }

    [Theory]
    [InlineData("claude", true)]
    [InlineData("CLAUDE", true)]
    [InlineData("codex", true)]
    [InlineData("agy", true)]
    [InlineData("cline", true)]
    [InlineData("nonexistent", false)]
    public void Find_handles_case_and_unknowns(string id, bool expectedFound)
    {
        var found = AgentClis.Find(id);
        Assert.Equal(expectedFound, found != null);
    }

    [Fact]
    public void GetIds_parses_comma_list_and_deduplicates()
    {
        var env = "# Config\nFRIDAY_AGENT_CLIS=claude, codex, claude, cline\nFRIDAY_CONTEXT_PROVIDER=gemini\n";
        var ids = AgentClis.GetIds(env);
        Assert.Equal(["claude", "codex", "cline"], ids);

        Assert.Empty(AgentClis.GetIds(""));
        Assert.Empty(AgentClis.GetIds("# comment only\n"));
    }

    [Fact]
    public void GetIds_handles_quoted_values()
    {
        var env = "FRIDAY_AGENT_CLIS=\"claude,codex\"\n";
        Assert.Equal(["claude", "codex"], AgentClis.GetIds(env));
    }

    [Fact]
    public void AddId_adds_to_empty_env()
    {
        var res = AgentClis.AddId("", "claude");
        Assert.Equal("FRIDAY_AGENT_CLIS=claude\n", res);
    }

    [Fact]
    public void AddId_appends_and_deduplicates()
    {
        var before = "FRIDAY_AGENT_CLIS=claude\n";
        var after = AgentClis.AddId(before, "codex");
        Assert.Equal("FRIDAY_AGENT_CLIS=claude,codex\n", after);

        var dup = AgentClis.AddId(after, "claude");
        Assert.Equal("FRIDAY_AGENT_CLIS=claude,codex\n", dup);
    }

    [Fact]
    public void AddId_preserves_other_settings_and_comments()
    {
        var before = "# comment\nGEMINI_API_KEY=abc123456\nFRIDAY_AGENT_CLIS=claude\nFRIDAY_CONTEXT_PROVIDER=gemini\n";
        var after = AgentClis.AddId(before, "cline");
        Assert.Contains("# comment", after);
        Assert.Contains("GEMINI_API_KEY=abc123456", after);
        Assert.Contains("FRIDAY_CONTEXT_PROVIDER=gemini", after);
        Assert.Contains("FRIDAY_AGENT_CLIS=claude,cline", after);
    }

    [Fact]
    public void RemoveId_removes_specified_id_keeping_others()
    {
        var before = "FRIDAY_AGENT_CLIS=claude,codex,agy\nFRIDAY_LISTEN=127.0.0.1:1\n";
        var after = AgentClis.RemoveId(before, "codex");
        Assert.Equal("FRIDAY_AGENT_CLIS=claude,agy\nFRIDAY_LISTEN=127.0.0.1:1\n", after);
    }

    [Fact]
    public void RemoveId_drops_line_when_last_id_removed()
    {
        var before = "# top\nFRIDAY_AGENT_CLIS=claude\nGEMINI_API_KEY=key\n";
        var after = AgentClis.RemoveId(before, "claude");
        Assert.DoesNotContain("FRIDAY_AGENT_CLIS", after);
        Assert.Contains("GEMINI_API_KEY=key", after);
        Assert.Contains("# top", after);
    }

    [Fact]
    public void RemoveId_noop_when_id_not_found()
    {
        var env = "FRIDAY_AGENT_CLIS=claude\n";
        Assert.Equal("FRIDAY_AGENT_CLIS=claude\n", AgentClis.RemoveId(env, "unknown"));
    }

    [Fact]
    public void FindOnPath_finds_exe_and_cmd_shims()
    {
        var tempDir = Directory.CreateTempSubdirectory("friday-path-test-").FullName;
        try
        {
            var claudeExe = Path.Combine(tempDir, "claude.exe");
            var codexCmd = Path.Combine(tempDir, "codex.cmd");
            var agyExe = Path.Combine(tempDir, "agy.exe");
            var clineCmd = Path.Combine(tempDir, "cline.cmd");

            File.WriteAllText(claudeExe, "");
            File.WriteAllText(codexCmd, "");
            File.WriteAllText(agyExe, "");
            File.WriteAllText(clineCmd, "");

            Assert.Equal(claudeExe, AgentClis.FindOnPath("claude", tempDir, ".exe;.cmd;.bat"));
            Assert.Equal(codexCmd, AgentClis.FindOnPath("codex", tempDir, ".exe;.cmd;.bat"));
            Assert.Equal(agyExe, AgentClis.FindOnPath("agy", tempDir, ".exe;.cmd;.bat"));
            Assert.Equal(clineCmd, AgentClis.FindOnPath("cline", tempDir, ".exe;.cmd;.bat"));
            Assert.Null(AgentClis.FindOnPath("nonexistent", tempDir, ".exe;.cmd;.bat"));
        }
        finally
        {
            try { Directory.Delete(tempDir, true); } catch { }
        }
    }

    [Fact]
    public async Task RunCheckAsync_returns_true_when_output_received_and_exit_code_zero()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return; // Unix shell script test

        var tempDir = Directory.CreateTempSubdirectory("friday-sh-test-").FullName;
        try
        {
            var script = Path.Combine(tempDir, "mock-agent.sh");
            File.WriteAllText(script, "#!/bin/sh\necho 'ready'\nexit 0\n");
            File.SetUnixFileMode(script, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);

            var cli = AgentClis.Find("claude")!;
            var ok = await AgentClis.RunCheckAsync(cli, script, timeout: TimeSpan.FromSeconds(5));
            Assert.True(ok);
        }
        finally
        {
            try { Directory.Delete(tempDir, true); } catch { }
        }
    }

    [Fact]
    public async Task RunCheckAsync_returns_false_when_command_fails_or_is_empty()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return;

        var tempDir = Directory.CreateTempSubdirectory("friday-sh-fail-").FullName;
        try
        {
            var failScript = Path.Combine(tempDir, "fail.sh");
            File.WriteAllText(failScript, "#!/bin/sh\nexit 1\n");
            File.SetUnixFileMode(failScript, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);

            var emptyScript = Path.Combine(tempDir, "empty.sh");
            File.WriteAllText(emptyScript, "#!/bin/sh\nexit 0\n");
            File.SetUnixFileMode(emptyScript, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);

            var cli = AgentClis.Find("claude")!;
            Assert.False(await AgentClis.RunCheckAsync(cli, failScript, timeout: TimeSpan.FromSeconds(5)));
            Assert.False(await AgentClis.RunCheckAsync(cli, emptyScript, timeout: TimeSpan.FromSeconds(5)));
        }
        finally
        {
            try { Directory.Delete(tempDir, true); } catch { }
        }
    }

    [Fact]
    public void First_provider_reads_free_models_with_kilo()
    {
        var first = Providers.All[0];
        Assert.Equal("opencode", first.Id);
        Assert.Equal("Free models: OpenCode Zen, then Kilo or OVHcloud (no key, no sign-in)", first.Name);
    }

    [Fact]
    public void An_npm_launcher_counts_as_installed()
    {
        var dir = Directory.CreateTempSubdirectory();
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "codex.cmd"), "");
            var codex = AgentClis.All.First(c => c.BinaryName == "codex");
            Assert.Equal(Path.Combine(dir.FullName, "codex.cmd"), AgentClis.FindCli(codex, dir.FullName));
        }
        finally
        {
            dir.Delete(true);
        }
    }

    [Fact]
    public void The_search_path_keeps_this_processs_path()
    {
        var first = (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator).First(d => d.Length > 0);
        Assert.Contains(first, AgentClis.SearchPath().Split(Path.PathSeparator));
    }
}
