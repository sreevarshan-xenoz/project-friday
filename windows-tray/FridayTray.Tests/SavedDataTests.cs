using FridayTray.Core;
using Xunit;

public class SavedDataTests : IDisposable
{
    readonly string _root = Path.Combine(Path.GetTempPath(), "friday-saved-" + Guid.NewGuid().ToString("N"));
    readonly Paths _paths;

    public SavedDataTests()
    {
        _paths = new Paths(Path.Combine(_root, "local"), Path.Combine(_root, "roaming"));
        Directory.CreateDirectory(_paths.MemoryDir);
    }

    public void Dispose() => Directory.Delete(_root, recursive: true);

    [Fact]
    public void Nothing_saved_counts_as_nothing()
    {
        var c = SavedData.Count(_paths);
        Assert.Equal(new SavedCounts(0, 0, 0, 0), c);
        Assert.False(c.Any);
    }

    [Fact]
    public void Counts_answers_by_marker_so_a_rule_inside_one_does_not_split_it()
    {
        var log = new AnswerLog(_paths);
        var ok = new AskOutcome(true, "Tabs first.\n\n---\n\nThen spaces.", null, false);
        log.Save("compare", null, ok, new DateTime(2026, 11, 15));
        log.Save("again", null, ok, new DateTime(2026, 11, 15));
        log.Remember("compare"); log.Remember("again");
        File.WriteAllText(_paths.EgressFile, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
        File.WriteAllText(Path.Combine(_paths.MemoryDir, "episodes.jsonl"), "{\"e\":1}\n");
        Assert.Equal(new SavedCounts(2, 2, 3, 1), SavedData.Count(_paths));
    }

    [Fact]
    public void Forget_deletes_all_of_it_and_leaves_state_json()
    {
        new AnswerLog(_paths).Save("q", null, new AskOutcome(true, "a", null, false), DateTime.Now);
        File.WriteAllText(_paths.EgressFile, "{}\n");
        File.WriteAllText(Path.Combine(_paths.MemoryDir, "episodes.jsonl"), "{}\n");
        File.WriteAllText(_paths.StateFile, "{}");
        Assert.Empty(SavedData.Forget(_paths));
        Assert.False(SavedData.Count(_paths).Any);
        Assert.False(File.Exists(_paths.LastAnswerFile));
        Assert.True(File.Exists(_paths.StateFile), "the daemon's live state is not saved data");
    }

    [Fact]
    public void Forget_names_a_file_it_could_not_delete()
    {
        File.WriteAllText(_paths.EgressFile, "{}\n");
        using var held = new FileStream(_paths.EgressFile, FileMode.Open, FileAccess.Read, FileShare.None);
        // Windows refuses to delete a file that is open; elsewhere it succeeds.
        var failed = SavedData.Forget(_paths);
        if (OperatingSystem.IsWindows())
            Assert.Contains("egress.jsonl", failed);
        else
            Assert.Empty(failed);
    }
}
