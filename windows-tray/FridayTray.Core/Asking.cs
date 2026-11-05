using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace FridayTray.Core;

public enum AskKind { Plain, Selection, Screen }

/// <summary>What the tray shows after an ask.</summary>
public sealed record AskOutcome(bool Ok, string Answer, string? Details, bool DaemonDown);

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
    public static string[] CtlArguments(AskKind kind, string question, string request) => kind == AskKind.Screen
        // Pressing the key and typing the question is the per-use consent;
        // the daemon's ContextGate still enforces it.
        ? ["context", "screen", "--yes", "--ask", question]
        : ["goal", "--wait", request];

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

    /// <summary>Turn friday-ctl's exit status and output into what the user sees.</summary>
    public static AskOutcome Interpret(AskKind kind, string question, bool exitOk, string output)
    {
        var objects = JsonObjects(output);
        string? result, error;
        if (kind == AskKind.Screen)
        {
            var last = objects.LastOrDefault();
            result = last.ValueKind == JsonValueKind.Object ? Str(last, "response") : null;
            error = last.ValueKind == JsonValueKind.Object ? Str(last, "error") : null;
        }
        else
        {
            var last = objects.LastOrDefault(o => o.TryGetProperty("outcome", out _));
            result = last.ValueKind == JsonValueKind.Object ? Str(last, "result") : null;
            error = last.ValueKind == JsonValueKind.Object ? Str(last, "error") ?? Str(last, "outcome") : null;
        }

        if (exitOk)
            return new AskOutcome(true, string.IsNullOrEmpty(result) ? $"Done: {question}" : result, null, false);

        var raw = string.IsNullOrEmpty(error) ? output.Trim() : error;
        var all = raw + output;
        if (all.Contains("FreeUsageLimitError"))
            return new AskOutcome(false, "OpenCode Zen's free limit is used up for now. Try again later, or add your own key (tray menu: AI provider).", raw, false);
        if (all.Contains("no token found") || all.Contains("cannot reach daemon") || all.Contains("Connection refused")
            || all.Contains("actively refused"))
            return new AskOutcome(false, "FRIDAY isn't running, so I couldn't ask. Start it and try again.", raw, true);
        if (all.Contains("timed out after"))
            return new AskOutcome(false, "This is taking longer than expected. FRIDAY may still be working on it: check the tray icon, or open the answers later.", raw, false);
        return new AskOutcome(false, raw.Length > 0 ? raw : "FRIDAY couldn't answer that.", null, false);
    }
}
