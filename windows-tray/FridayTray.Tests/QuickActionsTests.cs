using FridayTray.Core;
using Xunit;

public class QuickActionsTests
{
    [Fact]
    public void Without_a_prompts_file_there_are_the_four_defaults()
    {
        var all = QuickActions.All(Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")));
        Assert.Equal(QuickActions.Defaults, all);
        Assert.Equal(4, all.Count);
    }

    [Fact]
    public void Own_prompts_follow_the_defaults_skipping_comments_blanks_and_repeats()
    {
        var file = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));
        try
        {
            File.WriteAllText(file, "# mine\n\n  Make it formal  \nSummarize this\nList the action items\n");
            var all = QuickActions.All(file);
            Assert.Equal(["Make it formal", "List the action items"], all.Skip(4));
            Assert.Equal(6, all.Count);
        }
        finally { File.Delete(file); }
    }
}
