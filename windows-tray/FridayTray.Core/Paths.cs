namespace FridayTray.Core;

/// <summary>
/// Where FRIDAY keeps things for the current user on Windows. State (the
/// daemon's state.json, answers, logs) is per machine and goes under
/// %LOCALAPPDATA%; the AI key file is configuration and goes under %APPDATA%.
/// </summary>
public sealed class Paths
{
    public Paths(string localAppData, string appData)
    {
        StateDir = Path.Combine(localAppData, "friday");
        ConfigDir = Path.Combine(appData, "friday");
    }

    public static Paths ForCurrentUser() => new(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData));

    public string StateDir { get; }
    public string ConfigDir { get; }
    public string StateFile => Path.Combine(StateDir, "state.json");
    public string MemoryDir => Path.Combine(StateDir, "data");
    public string LogDir => Path.Combine(StateDir, "logs");
    public string HistoryFile => Path.Combine(StateDir, "ask-history");
    public string LastAnswerFile => Path.Combine(StateDir, "last-answer.md");
    public string AnswersFile => Path.Combine(StateDir, "answers.md");
    public string EgressFile => Path.Combine(StateDir, "egress.jsonl");
    public string ActionsFile => Path.Combine(StateDir, "actions.jsonl");
    public string StandingFile => Path.Combine(StateDir, "standing.json");
    public string StandingDueFile => Path.Combine(StateDir, "standing-due.jsonl");
    public string AgentsDir => Path.Combine(StateDir, "agents");
    public string HooksFile => Path.Combine(AgentsDir, "hooks.jsonl");
    public string EnvFile => Path.Combine(ConfigDir, "friday.env");
    /// <summary>What the person asked FRIDAY to remember (one "- " line each); every ask carries it.</summary>
    public string MemoryFile => Path.Combine(ConfigDir, "memory.md");
    public string NoAppContextFile => Path.Combine(ConfigDir, "no-app-context");
    public string PromptsFile => Path.Combine(ConfigDir, "prompts");
    public string PrivateWindowsFile => Path.Combine(ConfigDir, "private-windows");
    public string SensitiveWindowsFile => Path.Combine(ConfigDir, "sensitive-windows");
}
