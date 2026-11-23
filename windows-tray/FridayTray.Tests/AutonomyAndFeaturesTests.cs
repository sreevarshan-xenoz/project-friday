using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class AutonomyAndFeaturesTests
{
    private static Dictionary<string, string> Env(string text) => EnvFile.Parse(text);

    [Fact]
    public void Autonomy_is_ask_risky_unless_set_and_only_differences_are_written()
    {
        Assert.Equal("ask-risky", AppSettings.AutonomyMode(Env("")));
        Assert.Equal("ask-risky", AppSettings.AutonomyMode(Env("FRIDAY_AUTONOMY=whatever\n")));
        var act = AppSettings.WithAutonomy("", "act");
        Assert.Equal("act", AppSettings.AutonomyMode(Env(act)));
        Assert.Equal("ask-all", AppSettings.AutonomyMode(Env(AppSettings.WithAutonomy(act, "ask-all"))));
        Assert.DoesNotContain("FRIDAY_AUTONOMY", AppSettings.WithAutonomy(act, "ask-risky"));
    }

    [Fact]
    public void Only_act_confirms_by_itself_once_and_never_for_a_screen_ask()
    {
        Assert.True(AppSettings.AutoConfirm("act", screenAsk: false, alreadyConfirming: false));
        Assert.False(AppSettings.AutoConfirm("act", screenAsk: false, alreadyConfirming: true));
        Assert.False(AppSettings.AutoConfirm("act", screenAsk: true, alreadyConfirming: false));
        Assert.False(AppSettings.AutoConfirm("ask-risky", screenAsk: false, alreadyConfirming: false));
        Assert.False(AppSettings.AutoConfirm("ask-all", screenAsk: false, alreadyConfirming: false));
        // What can't be taken back still asks, in "act" too.
        Assert.False(AppSettings.AutoConfirm("act", screenAsk: false, alreadyConfirming: false, "desktop close_window {\"target\":\"notepad\"}"));
    }

    [Fact]
    public void A_features_list_with_spaces_and_empty_entries_still_reads()
    {
        Assert.False(AppSettings.FeatureOn(Env("FRIDAY_FEATURES_OFF= ,update-check ,, \n"), "update-check"));
        Assert.True(AppSettings.FeatureOn(Env("FRIDAY_FEATURES_OFF= ,update-check ,, \n"), "wrong-button"));
    }

    [Fact]
    public void Features_are_on_until_switched_off_one_by_one()
    {
        Assert.All(AppSettings.Features, f => Assert.True(AppSettings.FeatureOn(Env(""), f.Name)));
        var off = AppSettings.WithFeature("", "update-check", false);
        Assert.False(AppSettings.FeatureOn(Env(off), "update-check"));
        Assert.True(AppSettings.FeatureOn(Env(off), "wrong-button"));
        var both = AppSettings.WithFeature(off, "wrong-button", false);
        Assert.False(AppSettings.FeatureOn(Env(both), "wrong-button"));
        var back = AppSettings.WithFeature(AppSettings.WithFeature(both, "update-check", true), "wrong-button", true);
        Assert.DoesNotContain("FRIDAY_FEATURES_OFF", back);
        // Switching one off twice lists it once.
        Assert.Equal(off, AppSettings.WithFeature(off, "update-check", false));
    }
}
