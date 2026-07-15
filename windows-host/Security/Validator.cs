using WindowsHost.IPC;

namespace WindowsHost.Security;

/// <summary>
/// Validates incoming frames. This is transport/contract hardening only — it
/// makes NO permission decisions. Permission is enforced upstream by the
/// runtime's PermissionEngine before the intent ever reaches this host.
/// </summary>
public static class Validator
{
    public const byte ProtocolVersion = 1;

    /// <summary>Returns a normalized BridgeError code, or null if valid.</summary>
    public static string? Validate(BridgeFrame frame)
    {
        if (frame.ProtocolVersion != ProtocolVersion)
            return "ProtocolError";
        if (string.IsNullOrEmpty(frame.Message.Action) || !frame.Message.Action.StartsWith("windows."))
            return "ProtocolError";
        return null;
    }
}
