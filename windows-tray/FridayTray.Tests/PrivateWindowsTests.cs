using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class PrivateWindowsTests
{
    [Theory]
    [InlineData("KeePassXC", "Passwords.kdbx - KeePassXC", "keepass")]
    [InlineData("msedge", "New tab - [InPrivate] - Microsoft Edge", "inprivate")]
    [InlineData("chrome", "My Bank - Accounts - Google Chrome", "bank")]
    [InlineData("notepad", "notes.txt - Notepad", null)]
    public void The_built_in_list_matches_app_or_title(string app, string title, string? expected) =>
        Assert.Equal(expected, PrivateWindows.Match(app, title, null));

    [Fact]
    public void The_own_list_adds_and_drops()
    {
        var own = "# mine\nsalary\n!bank\n";
        Assert.Equal("salary", PrivateWindows.Match("excel", "salary 2026.xlsx", own));
        Assert.Null(PrivateWindows.Match("chrome", "Bank holidays - Wikipedia", own));
        Assert.Equal("keepass", PrivateWindows.Match("KeePass", "x", own));
    }
}
