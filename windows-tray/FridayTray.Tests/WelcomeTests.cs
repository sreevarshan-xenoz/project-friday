using FridayTray.Core;
using Xunit;

public class WelcomeTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "friday-welcome-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        if (Directory.Exists(_root))
            Directory.Delete(_root, recursive: true);
    }

    [Fact]
    public void The_welcome_is_pending_until_it_has_been_shown()
    {
        var paths = new Paths(Path.Combine(_root, "local"), Path.Combine(_root, "roaming"));
        Assert.True(Welcome.Pending(paths));
        Welcome.MarkShown(paths);
        Assert.False(Welcome.Pending(paths));
    }
}
