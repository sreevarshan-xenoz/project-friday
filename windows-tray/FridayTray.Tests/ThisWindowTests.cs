using FridayTray.Core;
using Xunit;

public class ThisWindowTests
{
    [Fact]
    public void Only_asks_about_this_window_name_it()
    {
        Assert.True(ThisWindow.Matches("close this window"));
        Assert.True(ThisWindow.Matches("Move the current app to the left"));
        Assert.False(ThisWindow.Matches("what is a window manager"));
    }

    [Fact]
    public void The_block_is_in_the_shape_the_daemon_reads_and_cant_be_broken_out_of()
    {
        Assert.Equal("\n\n[window in front when asked]\n\"this window\" is notepad (title: \"notes.txt - Notepad\"), even if another is in front now.",
            ThisWindow.Block("notepad", "notes.txt - Notepad"));
        var b = ThisWindow.Block("evil app\"", "a \"title\"\nnew line")!;
        Assert.Contains("is evilapp (title: \"a titlenew line\")", b);
        Assert.Null(ThisWindow.Block("  ", "x"));
        Assert.Null(ThisWindow.Block(null, "x"));
        var emoji = new string('a', 119) + "😀" + "tail";
        var cut = ThisWindow.Block("app", emoji)!;
        Assert.DoesNotContain("\ud83d\"", cut);
        Assert.Contains(new string('a', 119) + "\")", cut);
    }
}
