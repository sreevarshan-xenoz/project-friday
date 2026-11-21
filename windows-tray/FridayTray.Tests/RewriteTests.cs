using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class RewriteTests
{
    [Fact]
    public void Selected_text_offers_rewrites_first_copied_text_none()
    {
        var missing = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString());
        Assert.Equal("Rewrite: fix grammar and spelling", QuickActions.All(missing, selected: true)[0]);
        Assert.DoesNotContain(QuickActions.All(missing, selected: false), a => a.StartsWith("Rewrite:"));
    }

    [Fact]
    public void A_rewrite_question_says_how()
    {
        Assert.Equal("make it shorter", Rewrite.How("Rewrite: make it shorter"));
        Assert.Null(Rewrite.How("Summarize this"));
        Assert.Null(Rewrite.How("Rewrite: "));
        var request = Rewrite.Request("make it shorter", "teh text");
        Assert.Contains("Rewrite the text below: make it shorter.", request);
        Assert.EndsWith("\"\"\"\nteh text\n\"\"\"", request);
    }

    [Theory]
    [InlineData("The quick fox.", "The quick fox.")]
    [InlineData("\"The quick fox.\"", "The quick fox.")]
    [InlineData("```\nThe quick fox.\n```", "The quick fox.")]
    [InlineData("\"a\" and \"b\"", "\"a\" and \"b\"")]
    public void Wrapping_is_dropped(string answer, string expected) =>
        Assert.Equal(expected, Rewrite.Clean(answer));
}
