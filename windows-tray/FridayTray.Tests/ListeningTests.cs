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

public class VoiceAnswerTests
{
    [Theory]
    [InlineData(" Yes.", "run")]
    [InlineData("Run it!", "run")]
    [InlineData("Cancel.", "cancel")]
    [InlineData("“No.”", "cancel")]
    [InlineData("yes and delete the rest", null)]
    [InlineData("", null)]
    public void Only_a_whole_yes_or_no_answers(string said, string? expected) =>
        Assert.Equal(expected, Listening.YesNo(said));

    [Theory]
    [InlineData("close_window: File Explorer · Downloads", true)]
    [InlineData("ui_click: Send", true)]
    [InlineData("del /q C:\\build\\*", true)]
    [InlineData("git commit -m \"rm the old file\"", false)]
    [InlineData("move_window: Notepad to the left", false)]
    [InlineData("set_volume 30", false)]
    public void What_cant_be_undone_needs_a_click(string action, bool irreversible) =>
        Assert.Equal(irreversible, Listening.Irreversible(action));
}

public class FollowUpTests
{
    [Theory]
    [InlineData("Thanks.", true)]
    [InlineData("That's all", true)]
    [InlineData("stop", true)]
    [InlineData("and what about tomorrow?", false)]
    [InlineData("Thanks, and what about tomorrow?", false)]
    public void Some_words_end_the_conversation(string said, bool ends) =>
        Assert.Equal(ends, Listening.EndsConversation(said));
}

public class WakeTests
{
    [Theory]
    [InlineData("ask\twhat time is it", "ask", "what time is it")]
    [InlineData("followup\tand tomorrow?", "followup", "and tomorrow?")]
    [InlineData("note\tListening…", "note", "Listening…")]
    public void Wake_lines_say_what_to_do(string line, string kind, string text) =>
        Assert.Equal((kind, text), Listening.WakeLine(line));

    [Theory]
    [InlineData("ask\t  ")]
    [InlineData("run\trm -rf x")]
    [InlineData("no tab here")]
    public void Anything_else_is_ignored(string line) => Assert.Null(Listening.WakeLine(line));

    [Fact]
    public void Hey_Friday_is_off_until_turned_on()
    {
        var root = Directory.CreateTempSubdirectory();
        try
        {
            var paths = new Paths(root.FullName, root.FullName);
            Directory.CreateDirectory(Path.Combine(Listening.WhisperDir(paths), "Release"));
            File.WriteAllText(Path.Combine(Listening.WhisperDir(paths), "Release", "whisper-cli.exe"), "");
            File.WriteAllText(Path.Combine(Listening.WhisperDir(paths), "ggml-base.en.bin"), "");
            Assert.False(Listening.WakeOn(paths));
            Listening.SetWake(paths, true);
            Assert.True(Listening.WakeOn(paths));
            Listening.SetWake(paths, false);
            Assert.False(Listening.WakeOn(paths));
        }
        finally
        {
            root.Delete(true);
        }
    }
}
