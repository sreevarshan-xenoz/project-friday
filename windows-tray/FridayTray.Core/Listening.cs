namespace FridayTray.Core;

/// <summary>
/// Hold-to-talk: hold the ask key and speak, let go and the words are asked
/// (as hold Super+Y on Linux). `friday-ctl listen hold` records and turns the
/// speech into text on this PC with whisper.cpp, which `friday-ctl listen
/// setup` downloads; only the text is asked.
/// </summary>
public static class Listening
{
    /// <summary>Held longer than this, the key talks; a tap opens the ask box.</summary>
    public static readonly TimeSpan HoldDelay = TimeSpan.FromMilliseconds(400);

    public static string WhisperDir(Paths p) => Path.Combine(p.StateDir, "whisper");

    public static bool Installed(Paths p) =>
        File.Exists(Path.Combine(WhisperDir(p), "Release", "whisper-cli.exe"))
        && File.Exists(Path.Combine(WhisperDir(p), "ggml-base.en.bin"));

    /// <summary>What to tell the person when `listen hold` exits with <paramref name="exitCode"/>, or null when it heard something (0) or the key was only tapped (5).</summary>
    public static string? Problem(int exitCode, string key) => exitCode switch
    {
        0 or 5 => null,
        1 => $"I didn't catch that. Hold {key} and speak, or tap it to type.",
        3 => "Talking to FRIDAY isn't set up yet: Settings, Voice.",
        4 => "Couldn't open the microphone. Check that one is plugged in and that apps may use it (Windows Settings, Privacy, Microphone).",
        _ => "Listening didn't work this time. Tap the key to type instead.",
    };
}
