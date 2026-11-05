using System.Text.Json;

namespace FridayTray;

/// <summary>
/// A glance at the daemon's state.json for the tray icon and tooltip:
/// offline (no file), stopped, working on a goal, or ready.
/// </summary>
internal sealed record DaemonState(string Mode, string Detail)
{
    public static DaemonState Read(string stateFile)
    {
        try
        {
            if (!File.Exists(stateFile))
                return new("offline", "FRIDAY is offline");
            using var fs = new FileStream(stateFile, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var doc = JsonDocument.Parse(fs);
            var root = doc.RootElement;
            var status = root.TryGetProperty("status", out var s) ? s.GetString() ?? "" : "";
            if (status != "running")
                return new("stopped", $"FRIDAY is {(status == "" ? "stopped" : status)}");
            if (root.TryGetProperty("current", out var cur) && cur.ValueKind == JsonValueKind.Object
                && cur.TryGetProperty("active", out var active) && active.ValueKind == JsonValueKind.True)
            {
                var phase = cur.TryGetProperty("phase", out var ph) ? ph.GetString() : null;
                var goal = cur.TryGetProperty("goal", out var g) ? g.GetString() : null;
                return new("working", $"Working{(phase is null ? "" : " · " + phase)}{(goal is null ? "" : ": " + goal)}");
            }
            return new("ready", "FRIDAY is ready");
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
            return new("offline", "FRIDAY is offline");
        }
    }
}
