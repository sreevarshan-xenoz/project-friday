namespace FridayTray.Core;

/// <summary>
/// The ask history (questions only, newest first, no repeats, last 50) and
/// the answers on disk: the last one in last-answer.md, all of them appended
/// to answers.md. Same files and format as `friday ask` on Linux.
/// </summary>
public sealed class AnswerLog
{
    private readonly Paths _paths;
    private readonly Func<string, string> _mask;

    /// <param name="mask">Masks keys and passwords in text that is kept
    /// (friday-ctl mask: the daemon's own rule); none: kept as it is.</param>
    public AnswerLog(Paths paths, Func<string, string>? mask = null)
    {
        _paths = paths;
        _mask = mask ?? (s => s);
    }

    public IReadOnlyList<string> History() =>
        File.Exists(_paths.HistoryFile)
            ? File.ReadAllLines(_paths.HistoryFile).Where(l => l.Length > 0).ToList()
            : [];

    public void Remember(string question)
    {
        question = _mask(question);
        Directory.CreateDirectory(_paths.StateDir);
        var lines = new List<string> { question };
        lines.AddRange(History().Where(l => l != question));
        File.WriteAllLines(_paths.HistoryFile, lines.Take(50));
    }

    public static string Render(string question, string? context, AskOutcome outcome, DateTime when, bool screen = false)
    {
        var sb = new System.Text.StringBuilder();
        sb.Append($"# {question}\n\n_{when:yyyy-MM-dd HH:mm}_\n\n");
        // That there was text, never the text itself (a selection can be
        // anything: a message, a password manager's page).
        if (!string.IsNullOrWhiteSpace(context))
            sb.Append("> (with the text you asked about; not kept)\n\n");
        // A screen ask's capture (window text or a screenshot) went once and
        // was never written anywhere (tests/e2e/ask.sh checks that on Linux).
        else if (screen)
            sb.Append("> (with what was on your screen, sent this once; not kept)\n\n");
        sb.Append(outcome.Answer).Append('\n');
        if (!string.IsNullOrEmpty(outcome.Details))
            sb.Append($"\n<details>\n\n```\n{outcome.Details}\n```\n");
        return sb.ToString();
    }

    public void Save(string question, string? context, AskOutcome outcome, DateTime when, bool screen = false)
    {
        Directory.CreateDirectory(_paths.StateDir);
        var text = _mask(Render(question, context, outcome, when, screen));
        File.WriteAllText(_paths.LastAnswerFile, text);
        File.AppendAllText(_paths.AnswersFile, SavedData.AnswerMarker + "\n" + text + "\n---\n\n");
    }
}
