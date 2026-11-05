namespace FridayTray.Core;

/// <summary>
/// The ask history (questions only, newest first, no repeats, last 50) and
/// the answers on disk: the last one in last-answer.md, all of them appended
/// to answers.md. Same files and format as `friday ask` on Linux.
/// </summary>
public sealed class AnswerLog
{
    private readonly Paths _paths;

    public AnswerLog(Paths paths) => _paths = paths;

    public IReadOnlyList<string> History() =>
        File.Exists(_paths.HistoryFile)
            ? File.ReadAllLines(_paths.HistoryFile).Where(l => l.Length > 0).ToList()
            : [];

    public void Remember(string question)
    {
        Directory.CreateDirectory(_paths.StateDir);
        var lines = new List<string> { question };
        lines.AddRange(History().Where(l => l != question));
        File.WriteAllLines(_paths.HistoryFile, lines.Take(50));
    }

    public static string Render(string question, string? context, AskOutcome outcome, DateTime when)
    {
        var sb = new System.Text.StringBuilder();
        sb.Append($"# {question}\n\n_{when:yyyy-MM-dd HH:mm}_\n\n");
        if (!string.IsNullOrWhiteSpace(context))
        {
            var preview = context.Length > 300 ? context[..300] : context;
            sb.Append($"> {preview.Replace('\n', ' ').Replace('\r', ' ')}\n\n");
        }
        sb.Append(outcome.Answer).Append('\n');
        if (!string.IsNullOrEmpty(outcome.Details))
            sb.Append($"\n<details>\n\n```\n{outcome.Details}\n```\n");
        return sb.ToString();
    }

    public void Save(string question, string? context, AskOutcome outcome, DateTime when)
    {
        Directory.CreateDirectory(_paths.StateDir);
        var text = Render(question, context, outcome, when);
        File.WriteAllText(_paths.LastAnswerFile, text);
        File.AppendAllText(_paths.AnswersFile, text + "\n---\n\n");
    }
}
