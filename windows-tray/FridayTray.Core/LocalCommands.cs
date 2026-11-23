using System.Text.RegularExpressions;

namespace FridayTray.Core;

public enum LocalKind { Battery, Timer, Settings, Brief }

/// <summary>A request this PC answers by itself.</summary>
public sealed record LocalCommand(LocalKind Kind, int Seconds = 0, string Label = "", string Uri = "");

/// <summary>
/// Requests the tray handles with no AI and nothing sent anywhere, like
/// `friday_local_command` in packaging/linux/friday: the battery, a timer, and
/// the Wi-Fi, Bluetooth and sound settings. Anything else goes to the model.
/// </summary>
public static partial class LocalCommands
{
    public const int MaxTimerSeconds = 24 * 3600;

    [GeneratedRegex(@"^(battery|battery (level|status)|how much battery( is left| do i have)?)$")]
    private static partial Regex BatteryRx();

    [GeneratedRegex(@"^(set )?(a )?timer (for )?(\d+) ?(s|sec|secs|seconds?|m|min|mins|minutes?|h|hr|hours?)$")]
    private static partial Regex TimerRx();

    [GeneratedRegex(@"^(open )?(the )?(wi-?fi|network|bluetooth|sound) (settings|controls)$")]
    private static partial Regex SettingsRx();

    /// <summary>"What did I miss?": answered by friday-ctl brief from FRIDAY's own records.</summary>
    [GeneratedRegex(@"^((so )?what did i miss|catch me up|brief me|(my |the )?morning brief|good morning,?( friday)?)$")]
    private static partial Regex BriefRx();

    public static LocalCommand? Parse(string? question)
    {
        var q = (question ?? "").Trim().ToLowerInvariant().TrimEnd(' ', '\t', '?', '.', '!');
        if (q.Length == 0 || q.Length > 80)
            return null;
        if (BatteryRx().IsMatch(q))
            return new LocalCommand(LocalKind.Battery);
        if (BriefRx().IsMatch(q))
            return new LocalCommand(LocalKind.Brief);
        if (TimerRx().Match(q) is { Success: true } t)
        {
            if (!int.TryParse(t.Groups[4].Value, out var n) || n <= 0)
                return null;
            var unit = t.Groups[5].Value[0];
            var (secs, word) = unit switch
            {
                's' => (n, "second"),
                'm' => (n * 60, "minute"),
                _ => (n * 3600, "hour"),
            };
            if (secs > MaxTimerSeconds || n > 100_000)
                return null;
            return new LocalCommand(LocalKind.Timer, secs, n == 1 ? $"{n} {word}" : $"{n} {word}s");
        }
        if (SettingsRx().Match(q) is { Success: true } s)
        {
            var which = s.Groups[3].Value;
            return which switch
            {
                "bluetooth" => new LocalCommand(LocalKind.Settings, Label: "Bluetooth", Uri: "ms-settings:bluetooth"),
                "sound" => new LocalCommand(LocalKind.Settings, Label: "sound", Uri: "ms-settings:sound"),
                _ => new LocalCommand(LocalKind.Settings, Label: "Wi-Fi", Uri: "ms-settings:network-wifi"),
            };
        }
        return null;
    }

    /// <summary>The answer to a battery question. <paramref name="percent"/> is null with no battery.</summary>
    public static string BatteryAnswer(int? percent, bool pluggedIn, bool charging)
    {
        if (percent is not { } p || p is < 0 or > 100)
            return "This computer has no battery.";
        var state = charging ? "charging" : pluggedIn ? "plugged in" : "discharging";
        return $"Battery: {p}% ({state}).";
    }

    public static string TimerAnswer(LocalCommand c) => $"Timer set for {c.Label}.";

    public static string SettingsAnswer(LocalCommand c) => $"Opened {c.Label} settings.";
}
