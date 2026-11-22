using System.Text.Json;

namespace FridayTray.Core;

/// <summary>One preset rung sizing local AI can use, from `friday-ctl local-ai presets --json`.</summary>
public sealed record LocalAiPreset(
    string Id,
    string Label,
    long BudgetMb,
    string? Model,
    long DownloadMb,
    bool Vision,
    bool Fast,
    bool Available,
    string Note);

/// <summary>A model an engine on this PC already has.</summary>
public sealed record LocalModel(string Name, long SizeMb, bool Vision)
{
    public string Label => Name + (SizeMb > 0 ? $" · {SizeMb / 1024.0:0.0} GB" : "") + (Vision ? " · sees screenshots" : "");
}

/// <summary>Ollama or llama.cpp as found on this PC (`friday-ctl local-ai engines --json`).</summary>
public sealed record LocalEngine(string Engine, bool Installed, bool Running, string BaseUrl, IReadOnlyList<LocalModel> Models)
{
    public string Title => (Engine == "ollama" ? "Ollama" : "llama.cpp") + (Running ? " (running)" : " (installed, not running)");
}

/// <summary>
/// Parsed local AI recommendation combining machine detection and presets.
/// </summary>
public sealed record LocalAiInfo(
    string GpuName,
    long? VramMb,
    long RamMb,
    string? RecommendedPreset,
    string? RecommendedModel,
    bool Vision,
    string? Note)
{
    public string? Preset => RecommendedPreset;
    public string? Model => RecommendedModel;
    public bool HasModel => !string.IsNullOrEmpty(RecommendedModel);

    public string MachineSummary => LocalAi.FormatMachine(GpuName, VramMb, RamMb);

    public string VisionSummary => HasModel
        ? (Vision ? "sees screenshots" : "text only: can't see screenshots")
        : (Note ?? "too little memory for local AI; use a cloud provider");
}

/// <summary>
/// Pure, testable sizing and configuration for local AI on Windows.
/// Parses `friday-ctl local-ai detect` and `presets --json`, and updates `friday.env`.
/// </summary>
public static class LocalAi
{
    public const string DefaultBaseUrl = "http://127.0.0.1:11434/v1";

    /// <summary>
    /// Parses output of `friday-ctl local-ai detect` and `presets --json` into a <see cref="LocalAiInfo"/>.
    /// </summary>
    public static LocalAiInfo Parse(string detectOutput, string presetsJson)
    {
        var (gpu, _, vramMb, ramMb, _, _, _) = ParseDetect(detectOutput);
        var presets = ParsePresets(presetsJson);
        var recommended = PickRecommendedPreset(presets);

        var gpuName = FormatGpu(gpu);
        var model = recommended?.Model;
        var presetId = recommended?.Id;
        var vision = recommended?.Vision ?? false;
        var note = recommended?.Note;

        if (string.IsNullOrEmpty(model))
            note ??= "too little memory for local AI; use a cloud provider";

        return new LocalAiInfo(
            GpuName: gpuName,
            VramMb: vramMb,
            RamMb: ramMb,
            RecommendedPreset: presetId,
            RecommendedModel: model,
            Vision: vision,
            Note: note);
    }

    /// <summary>Parses key=value lines from `friday-ctl local-ai detect`.</summary>
    public static (string Gpu, bool Discrete, long? VramMb, long RamMb, long CpuThreads, bool Avx2, long DiskFreeMb) ParseDetect(string detectOutput)
    {
        var lines = detectOutput.Split('\n');
        string gpu = "none";
        bool discrete = false;
        long? vramMb = null;
        long ramMb = 0;
        long threads = 0;
        bool avx2 = false;
        long diskFree = 0;

        foreach (var raw in lines)
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#'))
                continue;
            var parts = line.Split('=', 2);
            if (parts.Length < 2)
                continue;
            var key = parts[0].Trim();
            var val = parts[1].Trim();

            switch (key)
            {
                case "gpu":
                    gpu = val;
                    break;
                case "gpu_discrete":
                    discrete = val is "yes" or "true" or "1";
                    break;
                case "vram_mb":
                    vramMb = long.TryParse(val, out var v) ? v : null;
                    break;
                case "ram_mb":
                    long.TryParse(val, out ramMb);
                    break;
                case "cpu_threads":
                    long.TryParse(val, out threads);
                    break;
                case "avx2":
                    avx2 = val is "yes" or "true" or "1";
                    break;
                case "disk_free_mb":
                    long.TryParse(val, out diskFree);
                    break;
            }
        }
        return (gpu, discrete, vramMb, ramMb, threads, avx2, diskFree);
    }

    /// <summary>Parses JSON array from `friday-ctl local-ai presets --json`.</summary>
    public static IReadOnlyList<LocalAiPreset> ParsePresets(string presetsJson)
    {
        if (string.IsNullOrWhiteSpace(presetsJson))
            return [];

        var trimmed = presetsJson.Trim();
        if (trimmed.StartsWith('['))
        {
            var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };
            return JsonSerializer.Deserialize<List<LocalAiPreset>>(trimmed, options) ?? [];
        }

        // Fallback for pipe-separated output: id|label|budget_mb|model|download_mb|vision|fast|available|note
        var list = new List<LocalAiPreset>();
        foreach (var raw in trimmed.Split('\n'))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#'))
                continue;
            var parts = line.Split('|');
            if (parts.Length < 9)
                continue;
            var id = parts[0].Trim();
            var label = parts[1].Trim();
            long.TryParse(parts[2].Trim(), out var budgetMb);
            var modelStr = parts[3].Trim();
            var model = string.IsNullOrEmpty(modelStr) ? null : modelStr;
            long.TryParse(parts[4].Trim(), out var dlMb);
            var vision = parts[5].Trim() is "1" or "true";
            var fast = parts[6].Trim() is "1" or "true";
            var available = parts[7].Trim() is "1" or "true";
            var note = parts[8].Trim();
            list.Add(new LocalAiPreset(id, label, budgetMb, model, dlMb, vision, fast, available, note));
        }
        return list;
    }

    /// <summary>
    /// The preset to set up: the GPU one when the model fits in video memory
    /// (fast), else Balanced, else any that fits, else Balanced (or the
    /// first) to explain why none does.
    /// </summary>
    public static LocalAiPreset? PickRecommendedPreset(IReadOnlyList<LocalAiPreset> presets)
    {
        bool Fits(LocalAiPreset p) => p.Available && !string.IsNullOrEmpty(p.Model);
        return presets.FirstOrDefault(p => p.Id == "gpu" && Fits(p))
            ?? presets.FirstOrDefault(p => p.Id == "balanced" && Fits(p))
            ?? presets.FirstOrDefault(Fits)
            ?? presets.FirstOrDefault(p => p.Id == "balanced")
            ?? presets.FirstOrDefault();
    }

    /// <summary>Formats GPU vendor or name into a clean label.</summary>
    public static string FormatGpu(string gpu) => gpu.Trim().ToLowerInvariant() switch
    {
        "nvidia" => "NVIDIA GPU",
        "amd" => "AMD GPU",
        "intel" => "Intel graphics",
        "none" => "None",
        _ => gpu.Trim()
    };

    /// <summary>Formats megabytes into GB string (e.g. "12 GB" or "15.4 GB").</summary>
    public static string FormatGb(long mb)
    {
        var g = mb / 1024.0;
        var round = Math.Round(g);
        if (Math.Abs(g - round) < 0.05)
            return $"{(long)round} GB";
        return $"{g:0.#} GB";
    }

    /// <summary>
    /// Formats hardware into a human-readable machine summary
    /// (e.g. "NVIDIA RTX 3060, 12 GB VRAM, 32 GB RAM" or "NVIDIA GPU, 15.4 GB RAM").
    /// </summary>
    public static string FormatMachine(string gpuName, long? vramMb, long ramMb)
    {
        var parts = new List<string>();
        var gpu = FormatGpu(gpuName);
        if (!string.Equals(gpu, "None", StringComparison.OrdinalIgnoreCase) &&
            !string.Equals(gpu, "No GPU", StringComparison.OrdinalIgnoreCase))
        {
            parts.Add(gpu);
        }
        if (vramMb is > 0)
        {
            parts.Add($"{FormatGb(vramMb.Value)} VRAM");
        }
        if (ramMb > 0)
        {
            parts.Add($"{FormatGb(ramMb)} RAM");
        }
        return string.Join(", ", parts);
    }

    /// <summary>Reads current Local AI state from friday.env variables.</summary>
    public static (bool IsOn, string? Model) GetCurrentState(IReadOnlyDictionary<string, string> env)
    {
        var on = env.TryGetValue("LLAMACPP_ENABLED", out var enabled) && enabled is "1" or "true";
        var model = env.TryGetValue("LLAMACPP_MODEL", out var m) && !string.IsNullOrWhiteSpace(m) ? m : null;
        return (on && model != null, model);
    }

    /// <summary>
    /// Builds `friday.env` content with local AI enabled and configured with the 5 required keys.
    /// Existing lines are preserved and previous values of the 5 keys are replaced.
    /// </summary>
    public static string WithLocalAi(string existingEnv, string model, bool vision, bool localFirst, string baseUrl = DefaultBaseUrl)
    {
        var text = existingEnv;
        text = EnvFile.WithSetting(text, "LLAMACPP_ENABLED", "1");
        text = EnvFile.WithSetting(text, "LLAMACPP_BASE_URL", baseUrl);
        text = EnvFile.WithSetting(text, "LLAMACPP_MODEL", model);
        text = EnvFile.WithSetting(text, "LLAMACPP_VISION", vision ? "1" : "0");
        text = EnvFile.WithSetting(text, "FRIDAY_LOCAL_FIRST", localFirst ? "1" : "0");
        return text;
    }

    /// <summary>A `friday-ctl local-ai pull` line "progress 42 1536 3482": (percent, MB in, MB in all), or null.</summary>
    public static (int Percent, long DoneMb, long TotalMb)? ParseProgress(string line)
    {
        var parts = line.Trim().Split(' ');
        return parts.Length == 4 && parts[0] == "progress"
            && int.TryParse(parts[1], out var p) && long.TryParse(parts[2], out var d) && long.TryParse(parts[3], out var t) && t > 0
            ? (Math.Clamp(p, 0, 100), d, t)
            : null;
    }

    /// <summary>"42% · 1.5 of 3.4 GB"</summary>
    public static string FormatProgress(int percent, long doneMb, long totalMb) =>
        $"{percent}% · {doneMb / 1024.0:0.0} of {totalMb / 1024.0:0.0} GB";

    /// <summary>The engines on this PC, from `friday-ctl local-ai engines --json` (none when it can't be read).</summary>
    public static IReadOnlyList<LocalEngine> ParseEngines(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.ValueKind != JsonValueKind.Array)
                return [];
            return doc.RootElement.EnumerateArray().Select(e => new LocalEngine(
                e.TryGetProperty("engine", out var n) ? n.GetString() ?? "" : "",
                e.TryGetProperty("installed", out var i) && i.ValueKind == JsonValueKind.True,
                e.TryGetProperty("running", out var r) && r.ValueKind == JsonValueKind.True,
                e.TryGetProperty("base_url", out var u) ? u.GetString() ?? DefaultBaseUrl : DefaultBaseUrl,
                e.TryGetProperty("models", out var ms) && ms.ValueKind == JsonValueKind.Array
                    ? ms.EnumerateArray().Select(m => new LocalModel(
                        m.TryGetProperty("name", out var mn) ? mn.GetString() ?? "" : "",
                        m.TryGetProperty("size_mb", out var sz) && sz.TryGetInt64(out var mb) ? mb : 0,
                        m.TryGetProperty("vision", out var v) && v.ValueKind == JsonValueKind.True))
                        .Where(m => m.Name.Length > 0).ToList()
                    : []))
                .Where(e => e.Engine is "ollama" or "llamacpp").ToList();
        }
        catch (JsonException)
        {
            return [];
        }
    }

    /// <summary>
    /// Builds `friday.env` content with local AI turned off (`LLAMACPP_ENABLED=0`).
    /// </summary>
    public static string TurnOffLocalAi(string existingEnv) =>
        EnvFile.WithSetting(existingEnv, "LLAMACPP_ENABLED", "0");
}
