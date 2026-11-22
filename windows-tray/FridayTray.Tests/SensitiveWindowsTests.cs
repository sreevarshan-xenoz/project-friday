using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class SensitiveWindowsTests
{
    [Theory]
    [InlineData("olk", "Inbox - Outlook", "olk")]
    [InlineData("chrome", "HTML element - MDN", null)]
    [InlineData("firefox", "Wi-Fi signal strength", null)]
    [InlineData("vlc", "folk songs.mp3 - VLC", null)]
    [InlineData("Discord", "#general - Discord", "discord")]
    [InlineData("ms-teams", "Chat | Microsoft Teams", "teams")]
    [InlineData("notepad", "notes.txt - Notepad", null)]
    public void Mail_and_chat_apps_match(string app, string title, string? expected) =>
        Assert.Equal(expected, SensitiveWindows.Match(app, title, null));

    [Fact]
    public void The_own_list_adds_and_drops()
    {
        var own = "# mine\nzulip\n!slack\n";
        Assert.Equal("zulip", SensitiveWindows.Match("Zulip", "streams", own));
        Assert.Equal("zulip", SensitiveWindows.Match("chrome", "general - Zulip", own));
        Assert.Null(SensitiveWindows.Match("slack", "Slack | general", own));
    }

    [Fact]
    public void Private_windows_keep_their_own_list() =>
        Assert.Null(PrivateWindows.Match("Discord", "#general - Discord", null));
}
