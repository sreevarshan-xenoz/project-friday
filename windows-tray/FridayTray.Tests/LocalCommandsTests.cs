using FridayTray.Core;
using Xunit;

public class LocalCommandsTests
{
    [Theory]
    [InlineData("battery")]
    [InlineData("Battery level?")]
    [InlineData("how much battery is left")]
    [InlineData("  how much battery do I have.  ")]
    public void Battery_questions_are_local(string q) =>
        Assert.Equal(LocalKind.Battery, LocalCommands.Parse(q)?.Kind);

    [Theory]
    [InlineData("timer 5 min", 300, "5 minutes")]
    [InlineData("set a timer for 1 minute", 60, "1 minute")]
    [InlineData("Timer for 30 seconds!", 30, "30 seconds")]
    [InlineData("set timer 2h", 7200, "2 hours")]
    [InlineData("a timer 1 hr", 3600, "1 hour")]
    public void Timers_know_their_length(string q, int seconds, string label)
    {
        var c = LocalCommands.Parse(q);
        Assert.Equal(LocalKind.Timer, c?.Kind);
        Assert.Equal(seconds, c!.Seconds);
        Assert.Equal(label, c.Label);
        Assert.Equal($"Timer set for {label}.", LocalCommands.TimerAnswer(c));
    }

    [Theory]
    [InlineData("timer 0 min")]
    [InlineData("timer 25 hours")]
    [InlineData("timer 99999999999 s")]
    [InlineData("timer five minutes")]
    public void Silly_timers_go_to_the_model_not_the_clock(string q) =>
        Assert.Null(LocalCommands.Parse(q));

    [Theory]
    [InlineData("wifi settings", "ms-settings:network-wifi")]
    [InlineData("open the Wi-Fi settings", "ms-settings:network-wifi")]
    [InlineData("network controls", "ms-settings:network-wifi")]
    [InlineData("bluetooth settings", "ms-settings:bluetooth")]
    [InlineData("sound settings", "ms-settings:sound")]
    public void Settings_open_the_right_page(string q, string uri) =>
        Assert.Equal(uri, LocalCommands.Parse(q)?.Uri);

    [Theory]
    [InlineData("what is a battery")]
    [InlineData("how do I make a timer in python")]
    [InlineData("battery life of the iphone")]
    [InlineData("explain wifi settings on routers")]
    [InlineData("")]
    [InlineData(null)]
    public void Everything_else_is_asked_of_the_model(string? q) =>
        Assert.Null(LocalCommands.Parse(q));

    [Fact]
    public void Battery_answers_say_what_the_battery_is_doing()
    {
        Assert.Equal("Battery: 80% (discharging).", LocalCommands.BatteryAnswer(80, false, false));
        Assert.Equal("Battery: 80% (charging).", LocalCommands.BatteryAnswer(80, true, true));
        Assert.Equal("Battery: 100% (plugged in).", LocalCommands.BatteryAnswer(100, true, false));
        Assert.Equal("This computer has no battery.", LocalCommands.BatteryAnswer(null, true, false));
        Assert.Equal("This computer has no battery.", LocalCommands.BatteryAnswer(255, true, false));
    }
}
