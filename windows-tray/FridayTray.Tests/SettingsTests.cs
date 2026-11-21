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
    public void Names_with_markup_characters_cannot_break_the_task_xml()
    {
        var xml = ElevatedStart.TaskXml(@"PC\a&b<c>", @"C:\Program Files\A&B\friday-tray.exe");
        var doc = System.Xml.Linq.XDocument.Parse(xml);
        var ns = System.Xml.Linq.XNamespace.Get("http://schemas.microsoft.com/windows/2004/02/mit/task");
        Assert.Equal(@"PC\a&b<c>", doc.Descendants(ns + "UserId").First().Value);
    }
}
