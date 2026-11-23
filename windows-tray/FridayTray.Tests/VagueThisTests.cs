using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class VagueThisTests
{
    [Theory]
    [InlineData("explain this")]
    [InlineData("What does this mean?")]
    [InlineData("is this right?")]
    [InlineData("translate this to French")]
    [InlineData("summarize that")]
    public void A_bare_this_asks_which(string question) => Assert.True(VagueThis.Matches(question));

    [Theory]
    [InlineData("what is this page about")]
    [InlineData("close this window")]
    [InlineData("is this application safe")]
    [InlineData("@screen what is this")]
    [InlineData("what is rust")]
    [InlineData("")]
    public void A_named_this_or_none_asks_nothing(string question) => Assert.False(VagueThis.Matches(question));
}
