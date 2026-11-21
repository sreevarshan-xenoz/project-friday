namespace FridayTray.Core;

/// <summary>
/// The on/off settings kept in friday.env, read and written the way the daemon
/// reads them (so the Settings window and `friday.env` never disagree):
/// `FRIDAY_DESKTOP=0` turns desktop control off, `FRIDAY_SCREEN_TEXT=0` turns
/// reading a window's text (instead of a screenshot) off. Only a value that differs from the default is
/// written, so a default install keeps an empty file.
/// </summary>
public static class AppSettings
{
    public const string DesktopControl = "FRIDAY_DESKTOP";
    public const string ScreenText = "FRIDAY_SCREEN_TEXT";
    public const string Agents = "FRIDAY_AGENTS";

    /// <summary>On unless explicitly "0" (the daemon's rule for settings that default on).</summary>
    public static bool DesktopControlOn(IReadOnlyDictionary<string, string> env) =>
        !env.TryGetValue(DesktopControl, out var v) || v != "0";

    /// <summary>On unless explicitly "0" (the daemon's rule for settings that default on).</summary>
    public static bool AgentsOn(IReadOnlyDictionary<string, string> env) =>
        !env.TryGetValue(Agents, out var v) || v != "0";

    /// <summary>On unless "0" or "false" (the daemon's rule for screen text).</summary>
    public static bool ScreenTextOn(IReadOnlyDictionary<string, string> env) =>
        !env.TryGetValue(ScreenText, out var v) || v is not ("0" or "false");

    /// <summary>No key or agent account in friday.env: FRIDAY is on the free models.</summary>
    public static bool UsingFreeModels(IReadOnlyDictionary<string, string> env) =>
        !env.Any(kv => !string.IsNullOrWhiteSpace(kv.Value)
            && (kv.Key.EndsWith("_API_KEY", StringComparison.Ordinal) || kv.Key == "FRIDAY_AGENT_CLIS"));

    public static string WithDesktopControl(string existing, bool on) =>
        EnvFile.WithSetting(existing, DesktopControl, on ? null : "0");

    public static string WithAgents(string existing, bool on) =>
        EnvFile.WithSetting(existing, Agents, on ? null : "0");

    public static string WithScreenText(string existing, bool on) =>
        EnvFile.WithSetting(existing, ScreenText, on ? null : "0");
}
