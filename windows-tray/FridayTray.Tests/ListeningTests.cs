using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class ListeningTests
{
    [Fact]
    public void Heard_is_no_problem() => Assert.Null(Listening.Problem(0, "Win+Y"));

    [Fact]
    public void A_tap_is_no_problem_either() => Assert.Null(Listening.Problem(5, "Win+Y"));

    [Theory]
    [InlineData(1, "didn't catch")]
    [InlineData(3, "isn't set up")]
    [InlineData(4, "microphone")]
    [InlineData(-1, "didn't work")]
    public void Each_failure_says_what_to_do(int code, string says) =>
        Assert.Contains(says, Listening.Problem(code, "Win+Y"));

    [Fact]
    public void Nothing_heard_names_the_key_that_was_held() =>
        Assert.Contains("Ctrl+Alt+Y", Listening.Problem(1, "Ctrl+Alt+Y"));

    [Fact]
    public void Installed_needs_both_whisper_and_the_model()
    {
        var root = Directory.CreateTempSubdirectory();
        try
        {
            var paths = new Paths(root.FullName, root.FullName);
            Assert.False(Listening.Installed(paths));
            Directory.CreateDirectory(Path.Combine(Listening.WhisperDir(paths), "Release"));
            File.WriteAllText(Path.Combine(Listening.WhisperDir(paths), "Release", "whisper-cli.exe"), "");
            Assert.False(Listening.Installed(paths));
            File.WriteAllText(Path.Combine(Listening.WhisperDir(paths), "ggml-base.en.bin"), "");
            Assert.True(Listening.Installed(paths));
        }
        finally
        {
            root.Delete(true);
        }
    }
}
