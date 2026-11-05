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
    public string EnvFile => Path.Combine(ConfigDir, "friday.env");
}
