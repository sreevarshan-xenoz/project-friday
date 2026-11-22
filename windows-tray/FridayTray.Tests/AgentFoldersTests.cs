using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class AgentFoldersTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "friday-folders-" + Guid.NewGuid());

    public AgentFoldersTests()
    {
        Directory.CreateDirectory(_dir);
    }

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    private static readonly char S = Path.DirectorySeparatorChar;

    [Fact]
    public void A_request_naming_an_agent_says_the_folder_it_last_used()
    {
        File.WriteAllText(Path.Combine(_dir, "last-folder.json"), $$"""{"opencode": "/home/me/code/site", "claude": "/srv/api"}""");
        var folders = AgentFolders.Load(_dir, "/home/me");
        Assert.Equal("opencode works in ~/code/site", folders.Chip("ask opencode to fix the tests"));
        Assert.Equal("opencode works in ~/code/site", folders.Chip("have Open Code add a README"));
        Assert.Equal("Claude Code works in /srv/api", folders.Chip("Claude Code, tidy the imports"));
        Assert.Equal($"Say which folder agy works in, like ~{S}code{S}site", folders.Chip("get antigravity to write docs"));
    }

    [Fact]
    public void No_chip_without_an_agent_or_once_a_folder_is_named()
    {
        File.WriteAllText(Path.Combine(_dir, "last-folder.json"), """{"opencode": "/home/me/code/site"}""");
        var folders = AgentFolders.Load(_dir, "/home/me");
        Assert.Null(folders.Chip("what's using my disk?"));
        Assert.Null(folders.Chip("ask opencode to fix the tests in ~/code/app"));
        Assert.Null(folders.Chip(@"ask opencode to fix the tests in C:\code\app"));
        Assert.Null(folders.Chip("ask opencode to fix the tests in /srv/app"));
        Assert.Null(folders.Chip("is the agyle board up?"));
        Assert.Null(folders.Chip("ask claude what a monad is"));
    }

    [Fact]
    public void Known_folders_come_from_runs_and_last_folders_once_each()
    {
        File.WriteAllText(Path.Combine(_dir, "r1.json"), """{"id": "r1", "folder": "/home/me/code/site", "started": 2}""");
        File.WriteAllText(Path.Combine(_dir, "r2.json"), """{"id": "r2", "folder": "/home/me/code/app", "started": 1}""");
        File.WriteAllText(Path.Combine(_dir, "last-folder.json"), """{"opencode": "/home/me/code/site", "agy": "/srv/api"}""");
        var folders = AgentFolders.Load(_dir, "/home/me");
        Assert.Equal(["/home/me/code/site", "/home/me/code/app", "/srv/api"], folders.Known);
    }

    [Fact]
    public void A_missing_or_broken_file_means_no_folders()
    {
        Assert.Empty(AgentFolders.Load(Path.Combine(_dir, "none"), "/home/me").Known);
        File.WriteAllText(Path.Combine(_dir, "last-folder.json"), "{not json");
        Assert.Empty(AgentFolders.Load(_dir, "/home/me").Last);
    }

    [Fact]
    public void Tilde_only_replaces_the_whole_home_folder()
    {
        var folders = new AgentFolders(new Dictionary<string, string>(), [], @"C:\Users\me");
        Assert.Equal(@"~\code\site", folders.Tilde(@"C:\Users\me\code\site"));
        Assert.Equal(@"C:\Users\meg\site", folders.Tilde(@"C:\Users\meg\site"));
        Assert.Equal(@"D:\work", folders.Tilde(@"D:\work"));
    }
}
