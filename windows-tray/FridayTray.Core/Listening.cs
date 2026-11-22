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

    /// <summary>"Hey Friday": the switch (off unless turned on) and whether it can run.</summary>
    public static string WakeFlag(Paths p) => Path.Combine(p.ConfigDir, "wake");

    public static bool WakeOn(Paths p) => File.Exists(WakeFlag(p)) && Installed(p);

    public static void SetWake(Paths p, bool on)
    {
        if (on)
        {
            Directory.CreateDirectory(p.ConfigDir);
            File.WriteAllText(WakeFlag(p), "");
        }
        else
        {
            try { File.Delete(WakeFlag(p)); } catch (IOException) { }
        }
    }

    /// <summary>A line from `friday-ctl listen wake`: (note|ask|followup, text), or null.</summary>
    public static (string Kind, string Text)? WakeLine(string line)
    {
        var tab = line.IndexOf('\t');
        if (tab <= 0)
            return null;
        var (kind, text) = (line[..tab], line[(tab + 1)..].Trim());
        return kind is "note" or "ask" or "followup" && text.Length > 0 ? (kind, text) : null;
    }

    public static bool Installed(Paths p) =>
        File.Exists(Path.Combine(WhisperDir(p), "Release", "whisper-cli.exe"))
        && File.Exists(Path.Combine(WhisperDir(p), "ggml-base.en.bin"));

    /// <summary>
    /// "run" or "cancel" when the words, all of them, are a yes or a no (as
    /// `friday_yes_no` on Linux); null otherwise.
    /// </summary>
    public static string? YesNo(string? said)
    {
        var q = (said ?? "").Trim().Trim(Punctuation).Trim().ToLowerInvariant();
        return q switch
        {
            "yes" or "y" or "yep" or "yeah" or "ok" or "okay" or "sure" or "go ahead" or "do it" or "run it" or "run" or "go" => "run",
            "no" or "n" or "nope" or "cancel" or "stop" or "don't" or "dont" or "don't do it" => "cancel",
            _ => null,
        };
    }

    /// <summary>"Stop", "no", "thanks", "that's all": said where a follow-up could go, the conversation ends.</summary>
    public static bool EndsConversation(string? said)
    {
        if (YesNo(said) == "cancel")
            return true;
        var q = (said ?? "").Trim().Trim(Punctuation).Trim().ToLowerInvariant();
        return q is "thanks" or "thank you" or "thanks friday" or "thank you friday" or "that's all" or "thats all"
            or "that's it" or "nothing" or "never mind" or "nevermind" or "bye" or "no thanks" or "no thank you";
    }

    private static readonly char[] Punctuation = ".,!?;:\"'“”‘’…-".ToCharArray();

    /// <summary>
    /// Can't be undone (as `friday_irreversible` on Linux): closing a window,
    /// clicking in an app, deleting, shutting down. A spoken "run" doesn't run
    /// these; a click does.
    /// </summary>
    public static bool Irreversible(string action)
    {
        var a = System.Text.RegularExpressions.Regex.Replace($" {action.ToLowerInvariant()} ", "\"[^\"]*\"|'[^']*'", "");
        return System.Text.RegularExpressions.Regex.IsMatch(a,
            @"(^|[\s;|&(`])(rm|rmdir|del|erase|rd|remove-item|format|shred|dd|mkfs[.a-z0-9]*|diskpart|sudo|runas|kill|killall|pkill|taskkill|stop-process|shutdown|reboot|restart-computer|stop-computer|poweroff|halt|truncate)\s")
            || System.Text.RegularExpressions.Regex.IsMatch(a,
                @"git +(push +.*(-f|--force)|reset +--hard|clean)|close_window|ui_click|> */");
    }

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
