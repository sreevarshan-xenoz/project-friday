namespace FridayTray.Core;

/// <summary>
/// Soft state sounds (off unless turned on, like <c>friday sounds on</c> on
/// Linux): Windows' own quiet cues for listening, an answer done, and one
/// that failed.
/// </summary>
public static class Sounds
{
    public static string Flag(Paths p) => Path.Combine(p.ConfigDir, "sounds");

    public static bool On(Paths p) => File.Exists(Flag(p));

    public static void Set(Paths p, bool on)
    {
        if (on)
        {
            Directory.CreateDirectory(p.ConfigDir);
            File.WriteAllText(Flag(p), "");
        }
        else
        {
            try { File.Delete(Flag(p)); } catch (IOException) { }
        }
    }

    /// <summary>The sound file for a cue in <paramref name="mediaDir"/> (C:\Windows\Media), or null.</summary>
    public static string? FileFor(string mediaDir, string cue)
    {
        var name = cue switch
        {
            "listening" => "Speech On.wav",
            "done" => "Windows Background.wav",
            "failed" => "Windows Foreground.wav",
            _ => null,
        };
        if (name is null)
            return null;
        var path = Path.Combine(mediaDir, name);
        return File.Exists(path) ? path : null;
    }
}
