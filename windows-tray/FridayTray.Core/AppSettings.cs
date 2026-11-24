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

    /// <summary>The model each job tries first, as "provider/model" (the
    /// usual ranking covers it when it fails); unset routes as always. Same
    /// variables as `friday model` on Linux.</summary>
    public static readonly (string Job, string Var, string What)[] ModelJobs =
    [
        ("Asks", "FRIDAY_MODEL_ASK", "Answers to what you ask, typed or spoken"),
        ("Screen", "FRIDAY_MODEL_SCREEN", "Questions about what's on the screen"),
        ("Chores", "FRIDAY_MODEL_CHORES", "Steps FRIDAY plans behind the scenes: desktop, agents, standing tasks"),
    ];

    /// <summary>Empty or "auto" (routed as always), or provider/model.</summary>
    public static bool IsModelPick(string pick)
    {
        var p = pick.Trim();
        if (p.Length == 0 || p == "auto")
            return true;
        var slash = p.IndexOf('/');
        return slash > 0 && slash < p.Length - 1 && !p.Any(char.IsWhiteSpace);
    }

    public static string WithModelPick(string existing, string variable, string pick)
    {
        var p = pick.Trim();
        return EnvFile.WithSetting(existing, variable, p.Length == 0 || p == "auto" ? null : p);
    }

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
        ("update-rollback", "Go back to the version I had when an update doesn't start properly"),
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
        ("live-web", "Questions about the weather, news, scores or prices search the web without \"search\" first"),
        ("backup", "Back up settings, memory and history, and restore one after checking it"),
        ("risk-table", "Settings shows what FRIDAY may do on its own, what asks first and what never runs"),
        ("offline-proof", "Settings proves what went off this PC lately and what FRIDAY is connected to now"),
        ("big-output-file", "A very long output is kept whole in a file named under the answer"),
        ("task-recipes", "Ready-made standing tasks in one click (Settings > Tasks)"),
        ("undo-file-writes", "What a file held before FRIDAY wrote it is kept for undo"),
        ("day-summary", "A short summary of a day by project, from FRIDAY's own logs, kept on this computer"),
        ("thread-compaction", "In a long conversation, older turns ride along as one short line each"),
        ("routine-offer", "After the same desktop steps three times, offer to keep them as a routine (Linux)"),
        ("event-tasks", "Standing tasks that start when a download finishes, a file changes or an app opens"),
        ("capture-receipt", "Before the first word, the answer card says what was sent: the app, its text and size or a screenshot, what was masked"),
        ("dictation-latch", "Tap the voice typing key twice to keep recording without holding it (Linux)"),
        ("past-answers", "\"What did you tell me about X last week\": past answers with their dates"),
        ("answer-cards", "Answers show tables and steps, and each code block gets its own Copy"),
        ("blank-secret-fields", "Password and card fields are painted over before a screenshot leaves"),
        ("config-import", "friday import: keys and a model from other assistants, previewed first (Linux)"),
        ("watch-until", "Tell me when a command works or a window closes, checked less often over time (Linux)"),
        ("upgrade-restore", "When an upgrade is undone, settings and memory go back too (Linux)"),
        ("mask-history", "Keys and passwords are masked in the questions and answers FRIDAY keeps"),
        ("log-chain", "Each action in the action log is chained to the one before, so a changed line shows"),
        ("standing-reversible", "A standing task runs only what can be undone; anything that would ask first is refused and logged"),
        ("memory-tidy", "When memory is fuller than an ask carries, offer a shorter list to approve"),
        ("activity", "One list of agent steps waiting for you, runs going now, standing tasks and runs finished lately"),
        ("model-per-job", "A model picked for asks, screen questions or chores (AI page) is tried first"),
        ("status-orb", "The answer card's orb shows listening, thinking and speaking"),
        ("screen-ocr", "A window with no text of its own is asked about with its words read off the screen here, not its picture"),
        ("usage-page", "Settings shows calls, failures, free-limit hits and speed per AI provider"),
        ("memory-changes", "A changed fact (\"I moved to Pune\") closes the old one with a date in the memory file"),
        ("brief", "\"What did I miss?\" answered from FRIDAY's own records (agents, standing tasks, reminders)"),
        ("call-pause", "\"Hey Friday\" rests while a call or meeting app uses the microphone"),
        ("action-check", "Before a risky action, say what will change; after it, check and say done, partly or failed"),
        ("extension-autoinstall", "Settings installs the browser extension for you (Developer mode, Load unpacked)"),
        ("speak-as-it-comes", "Spoken answers start with the first sentence while the rest is still coming"),
        ("app-instructions", "Your instructions for each app (app-instructions in the config folder) go with asks from it"),
        ("suggestions", "Asks that fit the app in front (a page, an email, a chat) are offered first in the ask box"),
        ("clipboard-expiry", "A clipboard copied over 10 minutes ago is not used as context for a new question"),
        ("screen-what-changed", "\"What changed?\" about the same window sends only the lines that changed since the last ask"),
        ("agent-compare", "Two agents on the same task, each in its own copy, compared before you keep one"),
        ("backup-new-home", "A backup restored on another computer points saved folders at the new home"),
    ];

    public static bool ExtensionAutoinstallOn(IReadOnlyDictionary<string, string> env) =>
        FeatureOn(env, "extension-autoinstall");

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
