using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class SettingsTests
{
    private static Dictionary<string, string> Env(string text) => EnvFile.Parse(text);

    [Fact]
    public void Defaults_are_desktop_control_on_and_screen_text_on()
    {
        var empty = Env("");
        Assert.True(AppSettings.DesktopControlOn(empty));
        Assert.True(AppSettings.AgentsOn(empty));
        Assert.True(AppSettings.ScreenTextOn(empty));
    }

    [Fact]
    public void Desktop_control_is_off_only_for_zero_like_the_daemon()
    {
        Assert.False(AppSettings.DesktopControlOn(Env("FRIDAY_DESKTOP=0")));
        Assert.True(AppSettings.DesktopControlOn(Env("FRIDAY_DESKTOP=1")));
        Assert.True(AppSettings.DesktopControlOn(Env("FRIDAY_DESKTOP=off")));
    }

    [Fact]
    public void Agents_is_off_only_for_zero()
    {
        Assert.False(AppSettings.AgentsOn(Env("FRIDAY_AGENTS=0")));
        Assert.True(AppSettings.AgentsOn(Env("FRIDAY_AGENTS=1")));
        Assert.True(AppSettings.AgentsOn(Env("FRIDAY_AGENTS=off")));
    }

    [Fact]
    public void Screen_text_is_off_only_for_zero_or_false()
    {
        Assert.True(AppSettings.ScreenTextOn(Env("FRIDAY_SCREEN_TEXT=1")));
        Assert.True(AppSettings.ScreenTextOn(Env("FRIDAY_SCREEN_TEXT=true")));
        Assert.False(AppSettings.ScreenTextOn(Env("FRIDAY_SCREEN_TEXT=0")));
        Assert.False(AppSettings.ScreenTextOn(Env("FRIDAY_SCREEN_TEXT=false")));
    }

    [Fact]
    public void Only_a_non_default_value_is_written_and_other_lines_are_kept()
    {
        var key = "OPENAI_API_KEY=sk-test\n";
        Assert.Equal(key, AppSettings.WithDesktopControl(key, true));
        var off = AppSettings.WithDesktopControl(key, false);
        Assert.Equal("OPENAI_API_KEY=sk-test\nFRIDAY_DESKTOP=0\n", off);
        Assert.Equal(key, AppSettings.WithDesktopControl(off, true));
        Assert.Equal(key, AppSettings.WithScreenText(key, true));
        var textOff = AppSettings.WithScreenText(key, false);
        Assert.Contains("FRIDAY_SCREEN_TEXT=0", textOff);
        Assert.Equal(key, AppSettings.WithScreenText(textOff, true));
        var agentsOff = AppSettings.WithAgents(key, false);
        Assert.Equal("OPENAI_API_KEY=sk-test\nFRIDAY_AGENTS=0\n", agentsOff);
        Assert.Equal(key, AppSettings.WithAgents(agentsOff, true));
    }

    [Fact]
    public void The_start_task_runs_the_tray_at_logon_at_the_highest_level_for_that_user()
    {
        var xml = ElevatedStart.TaskXml(@"PC\ada", @"C:\Users\ada\AppData\Local\Programs\FRIDAY\friday-tray.exe");
        var doc = System.Xml.Linq.XDocument.Parse(xml);
        var ns = System.Xml.Linq.XNamespace.Get("http://schemas.microsoft.com/windows/2004/02/mit/task");
        Assert.Equal("HighestAvailable", doc.Descendants(ns + "RunLevel").Single().Value);
        Assert.Equal(@"PC\ada", doc.Descendants(ns + "UserId").First().Value);
        Assert.Single(doc.Descendants(ns + "LogonTrigger"));
        Assert.Equal(@"C:\Users\ada\AppData\Local\Programs\FRIDAY\friday-tray.exe", doc.Descendants(ns + "Command").Single().Value);
        Assert.Equal("IgnoreNew", doc.Descendants(ns + "MultipleInstancesPolicy").Single().Value);
    }

    [Fact]
    public void The_task_runs_a_copy_only_administrators_can_change()
    {
        Assert.Equal(Path.Combine(@"C:\Program Files", "FRIDAY", "Administrator start"), ElevatedStart.MachineDir(@"C:\Program Files"));
        var root = Directory.CreateTempSubdirectory().FullName;
        var from = Path.Combine(root, "user-install");
        var to = Path.Combine(root, "machine-copy");
        Directory.CreateDirectory(Path.Combine(from, "shell"));
        File.WriteAllText(Path.Combine(from, "friday-tray.exe"), "tray");
        File.WriteAllText(Path.Combine(from, "shell", "x.dll"), "dll");
        Directory.CreateDirectory(to);
        File.WriteAllText(Path.Combine(to, "stale.exe"), "old");
        ElevatedStart.CopyInstall(from, to);
        Assert.Equal("tray", File.ReadAllText(Path.Combine(to, "friday-tray.exe")));
        Assert.Equal("dll", File.ReadAllText(Path.Combine(to, "shell", "x.dll")));
        Assert.False(File.Exists(Path.Combine(to, "stale.exe")), "the old copy is replaced, not merged");
        Assert.Equal(from, File.ReadAllText(Path.Combine(to, ElevatedStart.SourceFile)));
    }

    [Fact]
    public void An_update_since_the_copy_was_made_is_noticed()
    {
        var root = Directory.CreateTempSubdirectory().FullName;
        var from = Path.Combine(root, "user-install");
        var to = Path.Combine(root, "machine-copy");
        Directory.CreateDirectory(from);
        File.WriteAllText(Path.Combine(from, "friday-tray.exe"), "1.0");
        ElevatedStart.CopyInstall(from, to);
        string? Version(string f) => File.Exists(f) ? File.ReadAllText(f) : null;
        Assert.False(ElevatedStart.CopyIsOld(to, Version));
        File.WriteAllText(Path.Combine(from, "friday-tray.exe"), "1.1");
        Assert.True(ElevatedStart.CopyIsOld(to, Version));
        Assert.False(ElevatedStart.CopyIsOld(Path.Combine(root, "no-copy"), Version));
    }

    [Fact]
    public void Names_with_markup_characters_cannot_break_the_task_xml()
    {
        var xml = ElevatedStart.TaskXml(@"PC\a&b<c>", @"C:\Program Files\A&B\friday-tray.exe");
        var doc = System.Xml.Linq.XDocument.Parse(xml);
        var ns = System.Xml.Linq.XNamespace.Get("http://schemas.microsoft.com/windows/2004/02/mit/task");
        Assert.Equal(@"PC\a&b<c>", doc.Descendants(ns + "UserId").First().Value);
    }

    [Fact]
    public void A_model_pick_is_provider_slash_model_and_auto_clears_it()
    {
        Assert.True(AppSettings.IsModelPick("groq/llama-3.3-70b"));
        Assert.True(AppSettings.IsModelPick("openrouter/nvidia/nemotron:free"));
        Assert.True(AppSettings.IsModelPick(""));
        Assert.True(AppSettings.IsModelPick("auto"));
        foreach (var bad in new[] { "llama", "/x", "groq/", "groq/llama 3" })
            Assert.False(AppSettings.IsModelPick(bad), bad);

        var picked = AppSettings.WithModelPick("OPENROUTER_API_KEY=k\n", "FRIDAY_MODEL_ASK", " groq/llama-3.3-70b ");
        Assert.Equal("groq/llama-3.3-70b", Env(picked)["FRIDAY_MODEL_ASK"]);
        Assert.Equal("k", Env(picked)["OPENROUTER_API_KEY"]);
        Assert.False(Env(AppSettings.WithModelPick(picked, "FRIDAY_MODEL_ASK", "auto")).ContainsKey("FRIDAY_MODEL_ASK"));
        Assert.Equal(["FRIDAY_MODEL_ASK", "FRIDAY_MODEL_SCREEN", "FRIDAY_MODEL_CHORES"],
            AppSettings.ModelJobs.Select(j => j.Var).ToArray());
    }
}
