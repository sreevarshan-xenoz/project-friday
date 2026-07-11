using System.Text.Json;

namespace WindowsHost.IPC;

/// <summary>
/// Length-prefixed JSON framing: 4-byte little-endian length + UTF-8 JSON.
/// Matches the Rust <see cref="BridgeFrame"/> wire format exactly.
/// </summary>
public static class FrameCodec
{
    public static byte[] Encode(BridgeFrame frame)
    {
        return JsonSerializer.SerializeToUtf8Bytes(frame);
    }

    public static BridgeFrame Decode(byte[] json)
    {
        var frame = JsonSerializer.Deserialize<BridgeFrame>(json)
            ?? throw new InvalidOperationException("empty frame");
        return frame;
    }

    /// <summary>Build a correlated response frame from a request frame.</summary>
    public static BridgeFrame Respond(BridgeFrame request, BridgeResponse response)
    {
        var payload = JsonSerializer.SerializeToElement(response);
        return new BridgeFrame
        {
            ProtocolVersion = request.ProtocolVersion,
            Message = new ProtocolMessage
            {
                Id = Guid.NewGuid(),
                Source = "windows-host",
                Target = request.Message.Source,
                Action = request.Message.Action + ".response",
                Payload = payload,
                ReplyTo = request.Message.Id,
                Timestamp = DateTime.UtcNow,
            },
        };
    }
}
