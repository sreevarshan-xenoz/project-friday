using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace FridayTray.Core;

public enum AskKind { Plain, Selection, Screen }

/// <summary>
/// How a question is asked beyond its words: search the web first, or answer
/// again with more care from a model other than <see cref="Avoid"/> (the
/// "provider/model" that gave the answer being retried).
/// </summary>
public sealed record AskOptions(bool Web = false, bool Thorough = false, string? Avoid = null, string? Region = null);

/// <summary>A tool action pending explicit confirmation before execution.</summary>
public sealed record NeedsConfirmation(string Tool, string Action);

/// <summary>Details of the provider and model that served the answer.</summary>
public sealed record ServedByInfo(string Provider, string Model, long Ms);

/// <summary>What the tray shows after an ask.</summary>
public sealed record AskOutcome(
    bool Ok,
    string Answer,
    string? Details,
    bool DaemonDown,
    NeedsConfirmation? Confirmation = null,
    ServedByInfo? ServedBy = null,
    int Masked = 0)
{
    public NeedsConfirmation? NeedsConfirmation => Confirmation;
}

/// <summary>
/// The ask flow of `friday ask` (packaging/linux/friday), minus the desktop:
/// build the request, the friday-ctl arguments, and turn friday-ctl's output
/// into an answer or a plain-language failure.
/// </summary>
public static partial class Asking
{
    public const int MaxContextChars = 4000;

    /// <summary>The goal text sent to FRIDAY: the question, plus selected text as context.</summary>
    public static string BuildRequest(string question, string? context)
    {
        if (string.IsNullOrWhiteSpace(context))
            return question;
        var ctx = context.Length > MaxContextChars ? context[..MaxContextChars] : context;
        return $"{question}\n\nUse this text as context:\n\"\"\"\n{ctx}\n\"\"\"";
    }

    /// <summary>friday-ctl's arguments for this ask.</summary>
    public static string[] CtlArguments(AskKind kind, string question, string request) =>
        CtlArguments(kind, question, request, null);

    /// <summary>friday-ctl's arguments for this ask, optionally confirming an action.</summary>
    public static string[] CtlArguments(AskKind kind, string question, string request, string? confirm) =>
        CtlArguments(kind, question, request, confirm, null);

    /// <summary>friday-ctl's arguments for this ask, with its confirmation and options.</summary>
    public static string[] CtlArguments(AskKind kind, string question, string request, string? confirm, AskOptions? options)
    {
        // Pressing the key and typing the question is the per-use consent;
        // the daemon's ContextGate still enforces it.
        if (kind == AskKind.Screen)
        {
            var screen = new List<string> { "context", "screen", "--yes", "--ask", question, "--stream" };
            // An area the user dragged out: only that goes to the provider.
            if (!string.IsNullOrEmpty(options?.Region))
                screen.AddRange(["--region", options.Region]);
            return [.. screen];
        }
        var args = new List<string> { "goal", "--wait" };
        if (!string.IsNullOrEmpty(confirm))
            args.AddRange(["--confirm", confirm]);
        if (options?.Web == true)
            args.Add("--web");
        if (options?.Thorough == true)
        {
            args.Add("--thorough");
            if (!string.IsNullOrEmpty(options.Avoid) && options.Avoid.Contains('/'))
                args.AddRange(["--avoid", options.Avoid]);
        }
        args.Add(request);
        return [.. args];
    }

    /// <summary>
    /// A question that starts with "search the web for", "search" or "look up"
    /// asks for a web search (and only then: FRIDAY never searches on its
    /// own). Returns the question without that prefix.
    /// </summary>
    public static (string Question, bool Web) WebQuestion(string question)
    {
        var m = WebPrefix().Match(question.Trim());
        if (!m.Success || string.IsNullOrWhiteSpace(m.Groups[1].Value))
            return (question, false);
        return (m.Groups[1].Value.Trim(), true);
    }

    [GeneratedRegex(@"^(?:search\s+the\s+web\s+for|search|look\s+up)\s+(.+)$", RegexOptions.IgnoreCase | RegexOptions.Singleline)]
    private static partial Regex WebPrefix();

    public const int MaxFollowUpContextChars = 6000;

    /// <summary>The request for a follow-up: the question, with the answer it follows.</summary>
    public static string BuildFollowUp(string question, string previousAnswer)
    {
        var prev = previousAnswer.Length > MaxFollowUpContextChars ? previousAnswer[..MaxFollowUpContextChars] : previousAnswer;
        return $"{question}\n\nThis follows up on my previous question and your answer:\n\"\"\"\n{prev}\n\"\"\"";
    }

    /// <summary>The "provider/model" to avoid when asking again, from who answered.</summary>
    public static string? AvoidFor(ServedByInfo? served) =>
        served is null || string.IsNullOrEmpty(served.Provider) ? null : $"{served.Provider}/{served.Model}";

    /// <summary>
    /// The answer so far while <paramref name="request"/> is the goal in flight:
    /// state.json's <c>current.partial</c>, which grows as the answer streams.
    /// Null when there is none yet, the goal is another one, or it has ended.
    /// </summary>
    public static string? PartialAnswer(string stateJson, string request)
    {
        try
        {
            using var doc = JsonDocument.Parse(stateJson);
            if (!doc.RootElement.TryGetProperty("current", out var cur) || cur.ValueKind != JsonValueKind.Object)
                return null;
            if (!cur.TryGetProperty("goal", out var goal) || goal.GetString() != request)
                return null;
            if (!cur.TryGetProperty("active", out var active) || active.ValueKind != JsonValueKind.True)
                return null;
            if (!cur.TryGetProperty("partial", out var partial) || partial.ValueKind != JsonValueKind.String)
                return null;
            var text = partial.GetString();
            return string.IsNullOrWhiteSpace(text) ? null : text;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// The answer so far from one line of <c>friday-ctl --stream</c> output
    /// (<c>{"partial": "..."}</c>), or null for any other line.
    /// </summary>
    public static string? PartialLine(string line)
    {
        if (!line.StartsWith("{\"partial\"", StringComparison.Ordinal))
            return null;
        try
        {
            using var doc = JsonDocument.Parse(line);
            var text = doc.RootElement.GetProperty("partial").GetString();
            return string.IsNullOrWhiteSpace(text) ? null : text;
        }
        catch (Exception e) when (e is JsonException or KeyNotFoundException or InvalidOperationException)
        {
            return null;
        }
    }

    /// <summary>
    /// Clipboard text that looks like a password or token (one long run of
    /// non-space characters mixing letters and digits) must not be sent to a
    /// cloud model by accident.
    /// </summary>
    public static bool LooksLikeSecret(string text) =>
        TokenRun().IsMatch(text) && text.Any(char.IsLetter) && text.Any(char.IsDigit);

    [GeneratedRegex(@"^\S{12,}$")]
    private static partial Regex TokenRun();

    /// <summary>
    /// Every top-level JSON object in friday-ctl's output, which mixes
    /// pretty-printed objects with plain log lines.
    /// </summary>
    public static List<JsonElement> JsonObjects(string output)
    {
        var found = new List<JsonElement>();
        var bytes = Encoding.UTF8.GetBytes(output);
        var i = 0;
        while (i < bytes.Length)
        {
            var start = Array.IndexOf(bytes, (byte)'{', i);
            if (start < 0)
                break;
            try
            {
                var reader = new Utf8JsonReader(bytes.AsSpan(start), new JsonReaderOptions { AllowTrailingCommas = true });
                using var doc = JsonDocument.ParseValue(ref reader);
                found.Add(doc.RootElement.Clone());
                i = start + (int)reader.BytesConsumed;
            }
            catch (JsonException)
            {
                i = start + 1;
            }
        }
        return found;
    }

    static string? Str(JsonElement o, string name) =>
        o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    /// <summary>
    /// Parse friday-ctl output into a pending confirmation if the goal failed because
    /// a gated tool requires explicit confirmation before executing.
    /// </summary>
    public static NeedsConfirmation? ParseConfirmation(string output)
    {
        var objects = JsonObjects(output);
        foreach (var o in objects.AsEnumerable().Reverse())
        {
            if (o.TryGetProperty("needs_confirmation", out var nc) && nc.ValueKind == JsonValueKind.Object)
            {
                var tool = Str(nc, "tool");
                var action = Str(nc, "action");
                if (!string.IsNullOrEmpty(tool) && !string.IsNullOrEmpty(action))
                    return new NeedsConfirmation(tool, action);
            }
        }
        return null;
    }

    /// <summary>
    /// Parse friday-ctl exit code and output into a pending confirmation.
    /// Exit code 3 is distinct from failure 1 and indicates a tool needs confirmation.
    /// </summary>
    public static NeedsConfirmation? ParseConfirmation(int exitCode, string output)
    {
        // Only the daemon's structured needs_confirmation counts, never text.
        return exitCode == 3 ? ParseConfirmation(output) : null;
    }

    private static string? ExtractRawError(AskKind kind, string output)
    {
        var objects = JsonObjects(output);
        if (kind == AskKind.Screen)
        {
            var last = objects.LastOrDefault(o => !o.TryGetProperty("partial", out _));
            return last.ValueKind == JsonValueKind.Object ? Str(last, "error") : null;
        }
        else
        {
            var last = objects.LastOrDefault(o => o.TryGetProperty("outcome", out _) || o.TryGetProperty("error", out _));
            return last.ValueKind == JsonValueKind.Object ? Str(last, "error") ?? Str(last, "outcome") : null;
        }
    }

    /// <summary>Turn friday-ctl's exit status and output into what the user sees.</summary>
    public static AskOutcome Interpret(AskKind kind, string question, bool exitOk, string output)
    {
        if (!exitOk && ParseConfirmation(output) is { } conf)
        {
            var rawErr = ExtractRawError(kind, output);
            return new AskOutcome(false, $"FRIDAY wants to run: {conf.Action}", rawErr, false, conf);
        }
        return InterpretCore(kind, question, exitOk, output);
    }

    /// <summary>Turn friday-ctl's exit code and output into what the user sees.</summary>
    public static AskOutcome Interpret(AskKind kind, string question, int exitCode, string output)
    {
        if (exitCode == 3 && ParseConfirmation(exitCode, output) is { } conf)
        {
            var rawErr = ExtractRawError(kind, output);
            return new AskOutcome(false, $"FRIDAY wants to run: {conf.Action}", rawErr, false, conf);
        }
        return InterpretCore(kind, question, exitCode == 0, output);
    }

    private static AskOutcome InterpretCore(AskKind kind, string question, bool exitOk, string output)
    {
        var objects = JsonObjects(output);
        string? result, error;
        ServedByInfo? servedBy = null;
        int masked = 0;
        if (kind == AskKind.Screen)
        {
            // The result, not one of the {"partial": ...} lines before it.
            var last = objects.LastOrDefault(o => !o.TryGetProperty("partial", out _));
            result = last.ValueKind == JsonValueKind.Object ? Str(last, "response") : null;
            error = last.ValueKind == JsonValueKind.Object ? Str(last, "error") : null;
        }
        else
        {
            var last = objects.LastOrDefault(o => o.TryGetProperty("outcome", out _));
            result = last.ValueKind == JsonValueKind.Object ? Str(last, "result") : null;
            error = last.ValueKind == JsonValueKind.Object ? Str(last, "error") ?? Str(last, "outcome") : null;
            if (last.ValueKind == JsonValueKind.Object)
            {
                if (last.TryGetProperty("masked", out var mk) && mk.TryGetInt32(out var mc))
                    masked = mc;
                var sb = last.TryGetProperty("served_by", out var s) ? s : default;
                if (sb.ValueKind == JsonValueKind.Object)
                {
                    var provider = Str(sb, "provider");
                    var model = Str(sb, "model");
                    var ms = sb.TryGetProperty("ms", out var m) && m.TryGetInt64(out var v) ? v : 0;
                    if (!string.IsNullOrEmpty(provider))
                        servedBy = new ServedByInfo(provider, model ?? "", ms);
                }
            }
        }

        if (exitOk)
            return new AskOutcome(true, string.IsNullOrEmpty(result) ? $"Done: {question}" : result, null, false, Confirmation: null, ServedBy: servedBy, Masked: masked);

        var raw = string.IsNullOrEmpty(error) ? output.Trim() : error;
        var all = raw + output;
        if (all.Contains("FreeUsageLimitError"))
            return new AskOutcome(false, "OpenCode Zen's free limit is used up for now. Try again later, or add your own key (tray menu: AI provider).", raw, false, Masked: masked);
        // friday-ctl's own errors only: a provider's "connection refused"
        // (a local model server that isn't running) is not FRIDAY down.
        if (all.Contains("no token found") || all.Contains("cannot reach daemon"))
            return new AskOutcome(false, "FRIDAY isn't running, so I couldn't ask. Start it and try again.", raw, true, Masked: masked);
        if (all.Contains("timed out after"))
            return new AskOutcome(false, "This is taking longer than expected. FRIDAY may still be working on it: check the tray icon, or open the answers later.", raw, false, Masked: masked);
        return new AskOutcome(false, raw.Length > 0 ? raw : "FRIDAY couldn't answer that.", null, false, Masked: masked);
    }
}
