using System.Text.Json;

namespace FridayTray.Core;

/// <summary>
/// "That was wrong": a repro of the last answer to turn into a test, in
/// `repros` in FRIDAY's state folder, the same shape `friday wrong` keeps on
/// Linux. How it was asked, who answered and how fast, what went into it,
/// the reason and FRIDAY's version; never the question's or the answer's
/// text, and an error only as its kind.
/// </summary>
public static class Repro
{
    /// <summary>An error's kind (timeout, rate_limited, …), never its text.</summary>
    public static string? ErrorKind(string? error)
    {
        if (string.IsNullOrEmpty(error))
            return null;
        var e = error.ToLowerInvariant();
        if (e.Contains("timed out")) return "timeout";
        if (e.Contains("429") || e.Contains("rate limit") || e.Contains("ratelimit") || e.Contains("usage limit")) return "rate_limited";
        if (e.Contains("see images")) return "no_vision";
        if (e.Contains("reach") || e.Contains("connect") || e.Contains("network")) return "unreachable";
        if (e.Contains("confirm")) return "needs_confirmation";
        if (e.Contains("all candidates failed")) return "all_failed";
        return "other";
    }

    public static string Json(AskKind kind, AskOutcome outcome, int questionChars, string reason, string version, DateTimeOffset when) =>
        JsonSerializer.Serialize(new Dictionary<string, object?>
        {
            ["mode"] = kind.ToString().ToLowerInvariant(),
            ["ok"] = outcome.Ok,
            ["local"] = false,
            ["served_by"] = outcome.ServedBy is { } s
                ? new Dictionary<string, object> { ["provider"] = s.Provider, ["model"] = s.Model, ["ms"] = s.Ms }
                : null,
            ["used"] = outcome.Used ?? [],
            ["masked"] = outcome.Masked,
            ["error"] = outcome.Ok ? null : ErrorKind(outcome.Details ?? outcome.Answer),
            ["question_chars"] = questionChars,
            ["answer_chars"] = outcome.Answer.Length,
            ["reason"] = reason.Length > 120 ? reason[..120] : reason,
            ["version"] = version,
            ["ts"] = when.ToString("yyyy-MM-ddTHH:mm:sszzz"),
        });

    /// <summary>Saves it and returns its path.</summary>
    public static string Save(Paths paths, AskKind kind, AskOutcome outcome, int questionChars, string reason, string version)
    {
        var dir = Path.Combine(paths.StateDir, "repros");
        Directory.CreateDirectory(dir);
        var when = DateTimeOffset.Now;
        var file = Path.Combine(dir, $"{when:yyyyMMdd-HHmmss}.json");
        File.WriteAllText(file, Json(kind, outcome, questionChars, reason, version, when));
        return file;
    }
}
