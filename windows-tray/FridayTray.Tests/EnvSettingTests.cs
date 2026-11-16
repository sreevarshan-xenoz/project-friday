using FridayTray.Core;
using Xunit;

public class EnvSettingTests
{
    [Fact]
    public void A_setting_is_added_changed_and_removed_leaving_the_rest()
    {
        var env = "# FRIDAY AI provider credentials.\nOPENAI_API_KEY=sk-test\n";
        var on = EnvFile.WithSetting(env, "FRIDAY_SCREEN_TEXT", "1");
        Assert.Equal("# FRIDAY AI provider credentials.\nOPENAI_API_KEY=sk-test\nFRIDAY_SCREEN_TEXT=1\n", on);
        Assert.Equal("1", EnvFile.Parse(on)["FRIDAY_SCREEN_TEXT"]);

        var twice = EnvFile.WithSetting(on, "FRIDAY_SCREEN_TEXT", "1");
        Assert.Equal(on, twice);

        var off = EnvFile.WithSetting(on, "FRIDAY_SCREEN_TEXT", null);
        Assert.Equal(env, off);
    }

    [Fact]
    public void An_empty_file_and_windows_line_endings_are_fine()
    {
        Assert.Equal("FRIDAY_SCREEN_TEXT=1\n", EnvFile.WithSetting("", "FRIDAY_SCREEN_TEXT", "1"));
        Assert.Equal("", EnvFile.WithSetting("FRIDAY_SCREEN_TEXT=1\r\n", "FRIDAY_SCREEN_TEXT", null));
        Assert.Equal("A=1\nB=2\n", EnvFile.WithSetting("A=1\r\nB=3\r\n", "B", "2"));
    }
}
