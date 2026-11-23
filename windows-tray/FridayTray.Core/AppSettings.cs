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

    public const string AgentCheck = "FRIDAY_AGENT_CHECK";

    /// <summary>Off unless "1": FRIDAY runs the project's tests when an agent says it's done.</summary>
    public static bool AgentCheckOn(IReadOnlyDictionary<string, string> env) =>
        env.TryGetValue(AgentCheck, out var v) && v == "1";

    public const string AgentWorktree = "FRIDAY_AGENT_WORKTREE";

    /// <summary>Off unless "1": each agent run in a git folder works in its own worktree.</summary>
    public static bool AgentWorktreeOn(IReadOnlyDictionary<string, string> env) =>
        env.TryGetValue(AgentWorktree, out var v) && v == "1";

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

    /// <summary>How much FRIDAY may do on its own (`friday autonomy` on Linux):
    /// "ask-all" asks before every desktop action, "ask-risky" (the default)
    /// only before commands, files, launches and what can't be undone, "act"
    /// runs what it would have asked about and says so afterwards.</summary>
    public const string Autonomy = "FRIDAY_AUTONOMY";

    public static string AutonomyMode(IReadOnlyDictionary<string, string> env) =>
        env.TryGetValue(Autonomy, out var v) && v is "ask-all" or "act" ? v : "ask-risky";

    public static string WithAutonomy(string existing, string mode) =>
        EnvFile.WithSetting(existing, Autonomy, mode is "ask-all" or "act" ? mode : null);

    /// <summary>Whether the tray runs an action the daemon asked to confirm
    /// without asking: only in "act", only on the first answer (never one that
    /// was already a confirmed run, so it can't loop), and never for a screen
    /// ask (it carries no confirmation). What can't be taken back still asks,
    /// as on Linux.</summary>
    public static bool AutoConfirm(string mode, bool screenAsk, bool alreadyConfirming, string action = "") =>
        mode == "act" && !screenAsk && !alreadyConfirming && !Listening.Irreversible(action);

    /// <summary>The tray's features that can be switched off one by one, as
    /// (name, what it does). Off ones are listed in FRIDAY_FEATURES_OFF.</summary>
    public const string FeaturesOff = "FRIDAY_FEATURES_OFF";
    public static readonly (string Name, string What)[] Features =
    [
        ("screen-secret-check", "No screenshot leaves when the window shows a key or password (its text is checked here first)"),
        ("update-check", "Tell me when a newer FRIDAY is out (asks GitHub once a minute after sign-in)"),
        ("wrong-button", "\"That was wrong\" on answers (keeps how it was asked and answered, no text)"),
        ("couldnt-check", "Say when an answer comes from general knowledge, with nothing on this computer checked"),
        ("agent-resume", "A follow-up to a coding agent goes on in its session (\"tell opencode to also add tests\")"),
        ("self-docs", "Questions about FRIDAY are answered from its own guide"),
        ("sensitive-warn", "Mail and chat apps: the ask box says their text goes to your AI provider"),
        ("hold-to-talk", "Hold the ask key and speak (speech becomes text on this PC); a tap still opens the ask box"),
        ("voice-confirm", "Asked by voice: say run or cancel after the sound to answer a confirmation"),
        ("voice-followup", "Asked by voice: after a spoken answer and a sound, a follow-up needs no key"),
        ("which-this", "\"Explain this\" with nothing picked: ask whether it means the window in front"),
        ("dictation-cleanup", "Speech drops \"um\" and \"uh\" and uses your own words (dictionary in %APPDATA%\\friday)"),
        ("propose-memories", "\"I use fish …\" in a question: offer to remember it (nothing is kept unless you say so)"),
        ("memory-file", "Every ask carries what you asked FRIDAY to remember (memory.md in %APPDATA%\\friday)"),
        ("call-pause", "\"Hey Friday\" rests while a call or meeting app uses the microphone"),
    ];

    public static bool FeatureOn(IReadOnlyDictionary<string, string> env, string name) =>
        !env.TryGetValue(FeaturesOff, out var v)
        || !v.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).Contains(name);

    public static string WithFeature(string existing, string name, bool on)
    {
        var env = EnvFile.Parse(existing);
        var off = (env.TryGetValue(FeaturesOff, out var v) ? v : "")
            .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(n => n != name)
            .ToList();
        if (!on)
            off.Add(name);
        return EnvFile.WithSetting(existing, FeaturesOff, off.Count == 0 ? null : string.Join(",", off));
    }
}
