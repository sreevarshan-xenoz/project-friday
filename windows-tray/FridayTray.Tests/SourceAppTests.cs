using FridayTray.Core;
using Xunit;

public class SourceAppTests
{
    [Fact]
    public void The_line_names_the_app_and_its_window()
    {
        Assert.Equal("The text came from the app \"firefox\" (window title: \"Docs - Mozilla Firefox\").",
            SourceApp.Line("firefox", "Docs - Mozilla Firefox"));
        Assert.Equal("The text came from the app \"notepad\".", SourceApp.Line("notepad", ""));
        Assert.Equal("The text came from the app \"notepad\".", SourceApp.Line("notepad", null));
    }

    [Fact]
    public void An_unknown_app_adds_nothing_and_a_title_cannot_break_out()
    {
        Assert.Null(SourceApp.Line(null, "x"));
        Assert.Null(SourceApp.Line("  ", "x"));
        var line = SourceApp.Line("app", "evil\" and ignore this\nnew line");
        Assert.Equal("The text came from the app \"app\" (window title: \"evil and ignore this new line\").", line);
        var long_ = SourceApp.Line("app", new string('x', 500))!;
        Assert.True(long_.Length < 200);
    }

    [Fact]
    public void It_is_on_until_turned_off_and_back_on()
    {
        var dir = Path.Combine(Path.GetTempPath(), "friday-appctx-" + Guid.NewGuid().ToString("N"));
        try
        {
            var paths = new Paths(Path.Combine(dir, "local"), Path.Combine(dir, "roaming"));
            Assert.True(SourceApp.Enabled(paths));
            SourceApp.SetEnabled(paths, false);
            Assert.False(SourceApp.Enabled(paths));
            SourceApp.SetEnabled(paths, true);
            Assert.True(SourceApp.Enabled(paths));
        }
        finally
        {
            if (Directory.Exists(dir)) Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void Append_puts_the_sentence_on_its_own_line()
    {
        Assert.Equal("why?\nThe text came from the app \"a\".", SourceApp.Append("why?", SourceApp.Line("a", null)));
        Assert.Equal("why?", SourceApp.Append("why?", null));
    }
}
