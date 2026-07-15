using System.Text.Json;

namespace WindowsHost.IPC;

/// <summary>
/// Core correlated request/response envelope (mirrors friday_core::protocol::ProtocolMessage).
/// The C# host only ever speaks this shape over the pipe.
/// </summary>
public class ProtocolMessage
{
    public Guid Id { get; set; }
    public string Source { get; set; } = "windows-adapter";
    public string Target { get; set; } = "windows-host";
    public string Action { get; set; } = "";
    public JsonElement Payload { get; set; }
    public Guid? ReplyTo { get; set; }
    public DateTime Timestamp { get; set; }
}

/// <summary>
/// Versioned IPC frame. ProtocolVersion is the only bridge addition to the
/// core envelope, so the Rust contract stays frozen.
/// </summary>
public class BridgeFrame
{
    public byte ProtocolVersion { get; set; }
    public ProtocolMessage Message { get; set; } = new();
}

/// <summary>
/// The only response payload shape that crosses the boundary. Error is a
/// normalized BridgeError code (never an HRESULT / GetLastError value).
/// </summary>
public class BridgeResponse
{
    public bool Ok { get; set; }
    public JsonElement Output { get; set; }
    public string? Error { get; set; }
    // Optional host annotations (e.g. launch_resolution: New Instance /
    // Existing Instance / Failed). Not part of the frozen Rust contract shape
    // beyond being a free-form map, so it is safe to extend.
    public Dictionary<string, string>? Meta { get; set; }
}
