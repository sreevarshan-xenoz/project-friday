using FridayTray.Core;
using Xunit;

public class AppProfilesTests
{
    [Fact]
    public void An_apps_instruction_is_found_by_its_program_name()
    {
        var file = Path.Combine(Path.GetTempPath(), "friday-appinstr-" + Guid.NewGuid().ToString("N"));
        try
        {
            File.WriteAllText(file, "# mine\nWindowsTerminal: answer with commands only\nslack : short and friendly\nempty:\n");
            Assert.Equal("answer with commands only", AppInstructions.For(file, "windowsterminal"));
            Assert.Equal("short and friendly", AppInstructions.For(file, "Slack.exe"));
            Assert.Null(AppInstructions.For(file, "notepad"));
            Assert.Null(AppInstructions.For(file, "empty"));
            Assert.Null(AppInstructions.For(file, null));
            Assert.Null(AppInstructions.For(file + "-missing", "slack"));
            Assert.True(AppInstructions.Same("org.telegram.desktop", "desktop"));
            Assert.False(AppInstructions.Same("slack", "slacker"));
            Assert.Contains("[app instructions]\nIn slack, the person asked", AppInstructions.Append("hi", "slack", "be short"));
        }
        finally { File.Delete(file); }
    }

    [Fact]
    public void The_ask_box_offers_what_fits_the_app()
    {
        Assert.Equal(["Summarize this page"], AppSuggestions.For("msedge").Select(s => s.Label));
        Assert.Equal(["Summarize this email", "Draft a reply"], AppSuggestions.For("OUTLOOK.EXE").Select(s => s.Label));
        Assert.Equal(2, AppSuggestions.For("slack").Count);
        Assert.Empty(AppSuggestions.For("notepad"));
        Assert.Empty(AppSuggestions.For(null));
        var mail = AppSuggestions.For("thunderbird");
        Assert.Equal("Draft a short reply to this email, in my voice, ready to paste", AppSuggestions.Picked(mail, "Draft a reply")!.Question);
        Assert.Null(AppSuggestions.Picked(mail, "something else"));
    }
}
