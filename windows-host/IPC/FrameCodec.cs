using System.Text.Json;

namespace WindowsHost.IPC;

/// <summary>
/// Shared JSON contract. The Rust side (<c>serde</c> default) speaks
/// <c>snake_case</c>; the host is the adapter and must match it exactly so the
/// wire format is identical in both directions.
/// </summary>
public static class Serde
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    public static JsonElement ToElement<T>(T value) =>
        JsonSerializer.SerializeToElement(value, Options);
}

/// <summary>
/// Length-prefixed JSON framing: 4-byte little-endian length + UTF-8 JSON.
/// Matches the Rust <see cref="BridgeFrame"/> wire format exactly.
/// </summary>
public static class FrameCodec
{
    public static byte[] Encode(BridgeFrame frame)
    {
        return JsonSerializer.SerializeToUtf8Bytes(frame, Serde.Options);
    }

    public static BridgeFrame Decode(byte[] json)
    {
        var frame = JsonSerializer.Deserialize<BridgeFrame>(json, Serde.Options)
            ?? throw new InvalidOperationException("empty frame");
        return frame;
    }

    /// <summary>Build a correlated response frame from a request frame.</summary>
    public static BridgeFrame Respond(BridgeFrame request, BridgeResponse response)
    {
        // Error paths leave Output as a default (Undefined) JsonElement, which
        // cannot be serialized. Normalize it to null so the real error (in
        // `error`) always reaches the client instead of masking it.
        if (response.Output.ValueKind == JsonValueKind.Undefined)
        {
            response.Output = JsonDocument.Parse("null").RootElement;
        }
        var payload = JsonSerializer.SerializeToElement(response, Serde.Options);
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
