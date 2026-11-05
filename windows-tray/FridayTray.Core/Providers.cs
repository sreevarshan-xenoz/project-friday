using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>An AI provider FRIDAY can use, as `friday setup-ai` knows them.</summary>
public sealed record Provider(string Id, string Name, string KeyVar, bool Vision, bool KeyOptional);

public static partial class Providers
{
    public static readonly IReadOnlyList<Provider> All =
    [
        new("opencode", "OpenCode Zen (free, no key needed)", "OPENCODE_API_KEY", true, true),
        new("gemini", "Google Gemini (vision, free tier)", "GEMINI_API_KEY", true, false),
        new("groq", "Groq (fast, vision)", "GROQ_API_KEY", true, false),
        new("nvidia-nim", "NVIDIA NIM (vision)", "NVIDIA_NIM_API_KEY", true, false),
        new("openrouter", "OpenRouter (many models, no screen context)", "OPENROUTER_API_KEY", false, false),
        new("cerebras", "Cerebras (fast, no screen context)", "CEREBRAS_API_KEY", false, false),
    ];

    public static Provider? Find(string id) => All.FirstOrDefault(p => p.Id == id);

    /// <summary>The same shape check as `friday setup-ai`: no spaces or odd symbols.</summary>
    public static bool LooksLikeKey(string key) => KeyShape().IsMatch(key);

    [GeneratedRegex(@"^[A-Za-z0-9._:-]{8,}$")]
    private static partial Regex KeyShape();
}
