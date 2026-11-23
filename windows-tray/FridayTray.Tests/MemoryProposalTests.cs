using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class MemoryProposalTests
{
    [Theory]
    [InlineData("I use fish, how do I set an alias?", "you use fish")]
    [InlineData("my editor is Helix and it crashes", "your editor is helix")]
    [InlineData("how do i add a plugin, I am using neovim", "you use neovim")]
    [InlineData("I prefer dark mode. Which theme?", "you prefer dark mode")]
    [InlineData("I use fish\nhow do I alias", "you use fish")]
    public void What_a_question_says_about_you_is_offered(string question, string proposal) =>
        Assert.Equal(proposal, MemoryProposal.From(question));

    [Theory]
    [InlineData("what is rust")]
    [InlineData("I use it for work")]
    [InlineData("I use the terminal a lot")]
    [InlineData("I use abcd1234efgh5678ijkl")]
    [InlineData("I use api key abcd1234efgh5678ijkl, why 401?")]
    [InlineData("I run into permission errors")]
    public void Nothing_else_is(string question) => Assert.Null(MemoryProposal.From(question));

    [Fact]
    public void It_is_kept_in_your_own_words()
    {
        Assert.Equal("I use fish", MemoryProposal.InOwnWords("you use fish"));
        Assert.Equal("my editor is helix", MemoryProposal.InOwnWords("your editor is helix"));
        Assert.Equal("I'm on arch", MemoryProposal.InOwnWords("you're on arch"));
    }
}
