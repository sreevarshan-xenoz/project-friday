using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>An AI provider FRIDAY can use, as `friday setup-ai` knows them.</summary>
public sealed record Provider(string Id, string Name, string KeyVar, bool Vision, bool KeyOptional);

public static partial class Providers
{
    public static readonly IReadOnlyList<Provider> All =
    [
        new("opencode", "Free models: OpenCode Zen, then Kilo (no key, no sign-in)", "OPENCODE_API_KEY", true, true),
        new("gemini", "Google Gemini (vision, free tier)", "GEMINI_API_KEY", true, false),
        new("groq", "Groq (fast, vision)", "GROQ_API_KEY", true, false),
        new("nvidia-nim", "NVIDIA NIM (vision)", "NVIDIA_NIM_API_KEY", true, false),
        new("openrouter", "OpenRouter (many models, no screen context)", "OPENROUTER_API_KEY", false, false),
        new("cerebras", "Cerebras (fast, no screen context)", "CEREBRAS_API_KEY", false, false),
    ];

    public static Provider? Find(string id) => All.FirstOrDefault(p => p.Id == id);

    /// <summary>The same shape check as `friday setup-ai`: no spaces or odd symbols.</summary>
    public static bool LooksLikeKey(string key) => KeyShape().IsMatch(key);

    /// <summary>Map provider ids to readable display names.</summary>
    public static string DisplayName(string providerId) => providerId switch
    {
        "opencode-zen" => "OpenCode Zen",
        "kilo" => "Kilo",
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "cline" => "Cline",
        _ => providerId,
    };

    /// <summary>Format the attribution line, e.g. "via OpenCode Zen · 2.3 s".</summary>
    public static string FormatServedBy(string provider, long ms)
    {
        var name = DisplayName(provider);
        var seconds = (ms / 1000.0).ToString("0.0", System.Globalization.CultureInfo.InvariantCulture);
        return $"via {name} · {seconds} s";
    }

    [GeneratedRegex(@"^[A-Za-z0-9._:-]{8,}$")]
    private static partial Regex KeyShape();
}
