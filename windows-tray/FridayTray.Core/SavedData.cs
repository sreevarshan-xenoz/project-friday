namespace FridayTray.Core;

/// <summary>What FRIDAY keeps on this PC, counted.</summary>
public sealed record SavedCounts(int Answers, int Questions, int Sent, int Memory)
{
    public bool Any => Answers + Questions + Sent + Memory > 0;
}

/// <summary>
/// The data FRIDAY saves (answers, remembered questions, the log of what was
/// sent, the daemon's memory), counted and wiped. The same set as
/// `friday forget` on Linux. Counts only: contents are never read out.
/// </summary>
public static class SavedData
{
    /// <summary>Each saved answer starts with this line (as on Linux).</summary>
    public const string AnswerMarker = "<!-- friday answer -->";

    public static SavedCounts Count(Paths p) => new(
        Answers: Lines(p.AnswersFile, l => l == AnswerMarker),
        Questions: Lines(p.HistoryFile, l => l.Length > 0),
        Sent: Lines(p.EgressFile, l => l.Length > 0),
        Memory: Lines(Path.Combine(p.MemoryDir, "episodes.jsonl"), l => l.Length > 0));

    /// <summary>
    /// Delete it all. The daemon holds its memory journal open, so stop it
    /// first and restart it after. Returns the files that could not be deleted.
    /// </summary>
    public static IReadOnlyList<string> Forget(Paths p)
    {
        var failed = new List<string>();
        foreach (var file in new[]
        {
            p.AnswersFile, p.LastAnswerFile, p.HistoryFile, p.EgressFile,
            Path.Combine(p.MemoryDir, "episodes.jsonl"), p.MemoryFile,
            // The current conversation (friday-ctl and the daemon keep it).
            Path.Combine(p.StateDir, "conversations.json"), Path.Combine(p.StateDir, "conversation"),
        })
        {
            try
            {
                if (File.Exists(file))
                    File.Delete(file);
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                failed.Add(Path.GetFileName(file));
            }
        }
        return failed;
    }

    static int Lines(string file, Func<string, bool> counts)
    {
        try
        {
            return File.Exists(file) ? File.ReadLines(file).Count(counts) : 0;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return 0;
        }
    }
}
