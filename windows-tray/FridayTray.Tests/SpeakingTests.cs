using FridayTray.Core;
using Xunit;

public class SpeakingTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "friday-speaking-" + Guid.NewGuid().ToString("N"));
    private readonly Paths _paths;

    public SpeakingTests() => _paths = new Paths(Path.Combine(_root, "local"), Path.Combine(_root, "roaming"));

    public void Dispose()
    {
        if (Directory.Exists(_root))
            Directory.Delete(_root, recursive: true);
    }

    private void InstallVoice()
    {
        var dir = Speaking.VoiceDir(_paths);
        Directory.CreateDirectory(Path.Combine(dir, "bin"));
        Directory.CreateDirectory(Path.Combine(dir, "model"));
        File.WriteAllText(Path.Combine(dir, "bin", "sherpa-onnx-offline-tts.exe"), "");
        File.WriteAllText(Path.Combine(dir, "settings.json"), "{\"kind\": \"kitten\", \"speaker\": 0}");
    }

    [Fact]
    public void Nothing_is_spoken_until_the_voice_is_installed_even_if_turned_on()
    {
        Speaking.SetEnabled(_paths, true);
        Assert.False(Speaking.Installed(_paths));
        Assert.False(Speaking.Enabled(_paths));
        InstallVoice();
        Assert.True(Speaking.Enabled(_paths));
        Speaking.SetEnabled(_paths, false);
        Assert.False(Speaking.Enabled(_paths));
    }

    [Fact]
    public void Turning_it_off_when_it_was_never_on_is_fine()
    {
        Speaking.SetEnabled(_paths, false);
        Assert.False(File.Exists(Speaking.FlagFile(_paths)));
    }

    [Fact]
    public void The_chosen_voice_is_kept_and_the_installed_model_is_not_lost()
    {
        InstallVoice();
        Assert.Equal(0, Speaking.Speaker(_paths));
        Speaking.SetSpeaker(_paths, 4);
        Assert.Equal(4, Speaking.Speaker(_paths));
        Assert.Contains("\"kind\":\"kitten\"", File.ReadAllText(Path.Combine(Speaking.VoiceDir(_paths), "settings.json")));
    }

    [Fact]
    public void A_voice_outside_the_range_or_a_broken_settings_file_falls_back_safely()
    {
        InstallVoice();
        Speaking.SetSpeaker(_paths, 99);
        Assert.Equal(Speaking.Voices - 1, Speaking.Speaker(_paths));
        File.WriteAllText(Path.Combine(Speaking.VoiceDir(_paths), "settings.json"), "not json");
        Assert.Equal(0, Speaking.Speaker(_paths));
        Speaking.SetSpeaker(_paths, 2);
        Assert.Equal(2, Speaking.Speaker(_paths));
    }
}
