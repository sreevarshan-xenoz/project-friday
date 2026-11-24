using System.Text.Json.Nodes;

namespace FridayTray.Core;

/// <summary>
/// FRIDAY's spoken answers: whether the voice is installed (by `friday-ctl say
/// --setup`), whether it is turned on, and which of its voices is chosen. The
/// speaking itself is `friday-ctl say`; these are the files it and the tray share.
/// </summary>
public static class Speaking
{
    public const int Voices = 10;

    public static string VoiceDir(Paths p) => Path.Combine(p.StateDir, "voice");
    public static string FlagFile(Paths p) => Path.Combine(p.ConfigDir, "speak");
    private static string SettingsFile(Paths p) => Path.Combine(VoiceDir(p), "settings.json");

    public static bool Installed(Paths p) =>
        File.Exists(Path.Combine(VoiceDir(p), "bin", "sherpa-onnx-offline-tts.exe"))
        && Directory.Exists(Path.Combine(VoiceDir(p), "model"));

    /// <summary>On only when the person turned it on and the voice is there to speak.</summary>
    public static bool Enabled(Paths p) => File.Exists(FlagFile(p)) && Installed(p);

    public static void SetEnabled(Paths p, bool on)
    {
        if (on)
        {
            Directory.CreateDirectory(p.ConfigDir);
            File.WriteAllText(FlagFile(p), "");
        }
        else
        {
            try { File.Delete(FlagFile(p)); } catch (IOException) { }
        }
    }

    /// <summary>The names in `friday-ctl say --voices` output ("0 Bella", one per line), in order.</summary>
    public static IReadOnlyList<string> ParseVoices(string output) =>
        output.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(l => l.IndexOf(' ') is var at and > 0 && int.TryParse(l[..at], out _) ? l[(at + 1)..].Trim() : "")
            .Where(n => n.Length > 0)
            .ToList();

    /// <summary>The chosen voice, 0 to <see cref="Voices"/> - 1 (0 when none was chosen).</summary>
    public static int Speaker(Paths p)
    {
        try
        {
            var n = (int?)JsonNode.Parse(File.ReadAllText(SettingsFile(p)))?["speaker"] ?? 0;
            return Math.Clamp(n, 0, Voices - 1);
        }
        catch (Exception e) when (e is IOException or System.Text.Json.JsonException or InvalidCastException)
        {
            return 0;
        }
    }

    /// <summary>How much of an answer is said aloud (the voice's <c>length</c> setting).</summary>
    public static readonly IReadOnlyList<(string Value, string Label)> Lengths =
    [
        ("brief", "Brief: two sentences"),
        ("normal", "Normal: six sentences"),
        ("detailed", "Detailed: all of it"),
    ];

    public static string Length(Paths p)
    {
        try
        {
            var v = (string?)JsonNode.Parse(File.ReadAllText(SettingsFile(p)))?["length"];
            return Lengths.Any(l => l.Value == v) ? v! : "normal";
        }
        catch (Exception e) when (e is IOException or System.Text.Json.JsonException or InvalidOperationException)
        {
            return "normal";
        }
    }

    public static void SetLength(Paths p, string length)
    {
        JsonNode settings;
        try { settings = JsonNode.Parse(File.ReadAllText(SettingsFile(p))) ?? new JsonObject(); }
        catch (Exception e) when (e is IOException or System.Text.Json.JsonException) { settings = new JsonObject(); }
        settings["length"] = length;
        Directory.CreateDirectory(VoiceDir(p));
        File.WriteAllText(SettingsFile(p), settings.ToJsonString());
    }

    /// <summary>Chooses a voice, keeping the other settings (which model is installed).</summary>
    public static void SetSpeaker(Paths p, int speaker)
    {
        JsonNode settings;
        try { settings = JsonNode.Parse(File.ReadAllText(SettingsFile(p))) ?? new JsonObject(); }
        catch (Exception e) when (e is IOException or System.Text.Json.JsonException) { settings = new JsonObject(); }
        settings["speaker"] = Math.Clamp(speaker, 0, Voices - 1);
        Directory.CreateDirectory(VoiceDir(p));
        File.WriteAllText(SettingsFile(p), settings.ToJsonString());
    }
}

/// <summary>
/// An answer read aloud while it streams: each time more of it has arrived,
/// only the new part is sent to the voice. When the text no longer continues
/// what was sent (the answer was rewritten), nothing more is sent.
/// </summary>
public sealed class StreamedSpeech
{
    private string _sent = "";
    private bool _broken;

    /// <summary>The new text in <paramref name="soFar"/>, "" when nothing is new,
    /// or null when it doesn't continue what was sent.</summary>
    public string? Next(string soFar)
    {
        // Trailing spaces sent aren't part of what must match (the end of a
        // streamed piece can differ from the finished answer there).
        var sent = _sent.TrimEnd();
        if (_broken || !soFar.StartsWith(sent, StringComparison.Ordinal))
        {
            _broken = true;
            return null;
        }
        var add = soFar[sent.Length..];
        if (_sent.Length > sent.Length && add.StartsWith(_sent[sent.Length..], StringComparison.Ordinal))
            add = add[(_sent.Length - sent.Length)..];
        _sent = soFar;
        return add;
    }
}
