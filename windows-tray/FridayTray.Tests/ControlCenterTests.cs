using FridayTray.Core;
using Xunit;

public class ControlCenterTests
{
    [Fact]
    public void Free_models_until_a_key_or_account_is_set()
    {
        Assert.True(AppSettings.UsingFreeModels(new Dictionary<string, string>()));
        Assert.True(AppSettings.UsingFreeModels(new Dictionary<string, string> { ["FRIDAY_DESKTOP"] = "0" }));
        Assert.False(AppSettings.UsingFreeModels(new Dictionary<string, string> { ["GEMINI_API_KEY"] = "k" }));
        Assert.False(AppSettings.UsingFreeModels(new Dictionary<string, string> { ["FRIDAY_AGENT_CLIS"] = "claude-code" }));
        Assert.True(AppSettings.UsingFreeModels(new Dictionary<string, string> { ["GEMINI_API_KEY"] = "  " }));
    }

    [Fact]
    public void The_support_report_says_what_is_on_and_carries_no_secrets()
    {
        var text = SupportReport.Build("0.4.1", "Windows 11", "FRIDAY is ready", freeModels: false,
            desktopControl: true, screenText: false, voice: "on");
        Assert.Contains("FRIDAY: 0.4.1", text);
        Assert.Contains("AI: own account or key", text);
        Assert.Contains("Desktop control: on", text);
        Assert.Contains("Read a window's text: off", text);
        Assert.DoesNotContain("_API_KEY", text);
    }
}
