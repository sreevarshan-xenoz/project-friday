using System.Text.Json;

namespace WindowsHost;

/// <summary>
/// Loads the capability manifest served to the runtime on `windows.manifest`.
/// Keeps the host's advertised capabilities in one place (mirrors the Rust
/// static manifest in CAPABILITY_MANIFEST).
/// </summary>
public static class Manifest
{
    public static JsonElement Load()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "Manifest.json");
        using var doc = JsonDocument.Parse(File.ReadAllText(path));
        return doc.RootElement.Clone();
    }
}
