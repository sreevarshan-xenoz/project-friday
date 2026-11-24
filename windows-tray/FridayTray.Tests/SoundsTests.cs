using FridayTray.Core;
using Xunit;

public class SoundsTests
{
    [Fact]
    public void Sounds_are_off_until_turned_on_and_use_the_systems_own_files()
    {
        var root = Path.Combine(Path.GetTempPath(), "friday-sounds-" + Guid.NewGuid().ToString("N"));
        try
        {
            var p = new Paths(Path.Combine(root, "local"), Path.Combine(root, "roaming"));
            Assert.False(Sounds.On(p));
            Sounds.Set(p, true);
            Assert.True(Sounds.On(p));
            var media = Path.Combine(root, "media");
            Directory.CreateDirectory(media);
            File.WriteAllText(Path.Combine(media, "Speech On.wav"), "");
            Assert.EndsWith("Speech On.wav", Sounds.FileFor(media, "listening"));
            Assert.Null(Sounds.FileFor(media, "done"));
            Assert.Null(Sounds.FileFor(media, "nonsense"));
        }
        finally { try { Directory.Delete(root, true); } catch (IOException) { } }
    }
}
