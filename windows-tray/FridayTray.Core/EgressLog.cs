namespace FridayTray.Core;

using System.Text.Json;
using System.Text.Json.Serialization;

/// <summary>
/// One outbound request recorded in egress.jsonl.
/// NEVER carries prompt, answer, key, or any raw content.
/// </summary>
public sealed class EgressEntry
{
    [JsonPropertyName("ts")]
    public string Ts { get; set; } = "";

    [JsonPropertyName("goal_id")]
    public string? GoalId { get; set; }

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = "";

    [JsonPropertyName("model")]
    public string Model { get; set; } = "";

    [JsonPropertyName("kinds")]
    public List<string> Kinds { get; set; } = [];

    [JsonPropertyName("text_chars")]
    public int TextChars { get; set; }

    [JsonPropertyName("image_bytes")]
    public int ImageBytes { get; set; }

    [JsonPropertyName("masked")]
    public int? Masked { get; set; }

    [JsonPropertyName("outcome")]
    public string Outcome { get; set; } = "";

    [JsonPropertyName("local")]
    public bool? Local { get; set; }
}

/// <summary>
/// Parser and formatters for egress.jsonl ("What left this PC…").
/// </summary>
public static class EgressLog
{
    public static List<EgressEntry> Parse(string jsonl)
    {
        var entries = new List<EgressEntry>();
        if (string.IsNullOrWhiteSpace(jsonl))
            return entries;

        using var reader = new StringReader(jsonl);
        string? line;
        while ((line = reader.ReadLine()) != null)
        {
            var trimmed = line.Trim();
            if (trimmed.Length == 0)
                continue;
            try
            {
                var entry = JsonSerializer.Deserialize<EgressEntry>(trimmed);
                if (entry != null)
                    entries.Add(entry);
            }
            catch
            {
                // Ignore corrupted lines
            }
        }
        return entries;
    }

    public static IReadOnlyList<EgressEntry> ReadNewestFirst(string filePath)
    {
        if (!File.Exists(filePath))
            return [];
        try
        {
            var content = File.ReadAllText(filePath);
            var entries = Parse(content);
            entries.Reverse();
            return entries;
        }
        catch
        {
            return [];
        }
    }

    public static string FormatProvider(string provider) => Providers.DisplayName(provider);

    public static string FormatWhat(int textChars, int imageBytes)
    {
        var charsStr = textChars.ToString("N0", System.Globalization.CultureInfo.InvariantCulture);
        if (imageBytes >= 1024)
        {
            var kb = (imageBytes + 512) / 1024;
            return $"text {charsStr} chars + screenshot {kb:N0} KB";
        }
        if (imageBytes > 0)
        {
            return $"text {charsStr} chars + screenshot {imageBytes:N0} B";
        }
        return $"text {charsStr} chars";
    }

    public static string FormatLocalTime(string tsUtc)
    {
        if (DateTimeOffset.TryParse(tsUtc, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeUniversal, out var dto))
        {
            return dto.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss");
        }
        return tsUtc;
    }

    public static string FormatSummary(IReadOnlyList<EgressEntry> entries, DateTime nowUtc)
    {
        if (entries.Count == 0)
            return "Nothing has been sent yet.";

        var cutoff = nowUtc.AddDays(-7);
        var recent = entries.Where(e =>
        {
            if (DateTimeOffset.TryParse(e.Ts, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeUniversal, out var dto))
            {
                return dto.UtcDateTime >= cutoff;
            }
            return false;
        }).ToList();

        if (recent.Count == 0)
            return "0 requests in the last 7 days.";

        var total = recent.Count;
        var screenshots = recent.Count(e => e.ImageBytes > 0 || e.Kinds.Contains("image"));
        var maskedSum = recent.Sum(e => e.Masked ?? 0);

        var byProvider = recent
            .GroupBy(e => FormatProvider(e.Provider))
            .Select(g => new { Name = g.Key, Count = g.Count() })
            .OrderByDescending(p => p.Count)
            .ThenBy(p => p.Name)
            .Select(p => $"{p.Count} {p.Name}")
            .ToList();

        var provStr = string.Join(", ", byProvider);
        var reqWord = total == 1 ? "request" : "requests";
        var summary = $"{total} {reqWord} in the last 7 days: {provStr}";

        if (screenshots > 0)
            summary += $"; {screenshots} with a screenshot";

        if (maskedSum > 0)
        {
            var secWord = maskedSum == 1 ? "secret" : "secrets";
            summary += $"; {maskedSum} {secWord} masked";
        }

        return summary;
    }
}
