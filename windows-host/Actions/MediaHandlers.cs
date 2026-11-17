using System.Text.Json;
using System.Runtime.Versioning;
using Windows.Media.Control;
using WindowsHost.IPC;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

/// <summary>
/// Music and video, through Windows' media session (the same one the volume
/// flyout and a keyboard's media keys talk to): whatever app is playing,
/// with exact play and pause, not a toggle.
/// </summary>
[SupportedOSPlatform("windows10.0.19041.0")]
internal static class MediaSession
{
    public static GlobalSystemMediaTransportControlsSession? Current() =>
        GlobalSystemMediaTransportControlsSessionManager.RequestAsync().AsTask().GetAwaiter().GetResult()
            .GetCurrentSession();

    public static T Wait<T>(Windows.Foundation.IAsyncOperation<T> op) => op.AsTask().GetAwaiter().GetResult();
}

[SupportedOSPlatform("windows10.0.19041.0")]
public class MediaHandler : IActionHandler
{
    public string Intent => "windows.media";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var command = payload.GetProperty("command").GetString()
            ?? throw new ArgumentException("missing command");
        var session = MediaSession.Current();
        if (session is null)
            return new BridgeResponse { Ok = false, Error = "Nothing is playing: no app is offering media controls" };

        bool accepted = command switch
        {
            "play" => MediaSession.Wait(session.TryPlayAsync()),
            "pause" => MediaSession.Wait(session.TryPauseAsync()),
            "play-pause" => MediaSession.Wait(session.TryTogglePlayPauseAsync()),
            "next" => MediaSession.Wait(session.TrySkipNextAsync()),
            "previous" => MediaSession.Wait(session.TrySkipPreviousAsync()),
            "stop" => MediaSession.Wait(session.TryStopAsync()),
            _ => throw new ArgumentException($"unknown media command \"{command}\""),
        };
        return accepted
            ? new BridgeResponse { Ok = true, Output = Serde.ToElement(new { media = command }) }
            : new BridgeResponse { Ok = false, Error = $"The app playing didn't accept \"{command}\"" };
    }
}

[SupportedOSPlatform("windows10.0.19041.0")]
public class NowPlayingHandler : IActionHandler
{
    public string Intent => "windows.nowplaying";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var session = MediaSession.Current();
        if (session is null)
            return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { playing = false }) };
        var props = MediaSession.Wait(session.TryGetMediaPropertiesAsync());
        var status = session.GetPlaybackInfo().PlaybackStatus.ToString().ToLowerInvariant();
        return new BridgeResponse
        {
            Ok = true,
            Output = Serde.ToElement(new { playing = status == "playing", status, title = props.Title, artist = props.Artist }),
        };
    }
}

[SupportedOSPlatform("windows")]
public class VolumeHandler : IActionHandler
{
    public string Intent => "windows.volume";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        try
        {
            (int Level, bool Muted) now;
            if (payload.TryGetProperty("level", out var level) && level.ValueKind == JsonValueKind.Number)
            {
                var percent = level.GetInt32();
                if (percent < 0 || percent > 100)
                    throw new ArgumentException("level is a number from 0 to 100");
                now = CoreAudio.SetLevel(percent);
            }
            else if (payload.TryGetProperty("mute", out var mute) && mute.ValueKind == JsonValueKind.String)
            {
                now = mute.GetString() switch
                {
                    "on" => CoreAudio.SetMute(true),
                    "off" => CoreAudio.SetMute(false),
                    "toggle" => CoreAudio.SetMute(null),
                    var other => throw new ArgumentException($"mute is on, off or toggle, not \"{other}\""),
                };
            }
            else
            {
                now = CoreAudio.Read();
            }
            return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { level = now.Level, muted = now.Muted }) };
        }
        catch (CoreAudio.NoOutputException e)
        {
            return new BridgeResponse { Ok = false, Error = e.Message };
        }
    }
}

[SupportedOSPlatform("windows")]
public class BrightnessHandler : IActionHandler
{
    public string Intent => "windows.brightness";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        try
        {
            if (payload.TryGetProperty("level", out var level) && level.ValueKind == JsonValueKind.Number)
            {
                var percent = level.GetInt32();
                if (percent < 1 || percent > 100)
                    throw new ArgumentException("level is a number from 1 to 100");
                return Ok(Brightness.Set(percent));
            }
            return Ok(Brightness.Read());
        }
        catch (Brightness.NoPanelException e)
        {
            return new BridgeResponse { Ok = false, Error = e.Message };
        }

        static BridgeResponse Ok(int level) =>
            new() { Ok = true, Output = Serde.ToElement(new { level }) };
    }
}

[SupportedOSPlatform("windows")]
public class DesktopsHandler : IActionHandler
{
    public string Intent => "windows.desktops";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        try
        {
            var (count, current) =
                payload.TryGetProperty("to", out var to) && to.ValueKind == JsonValueKind.Number
                    ? VirtualDesktops.Go(to.GetInt32(), 0)
                : payload.TryGetProperty("by", out var by) && by.ValueKind == JsonValueKind.Number
                    ? VirtualDesktops.Go(null, by.GetInt32())
                    : VirtualDesktops.Read();
            return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { count, current }) };
        }
        catch (ArgumentException e)
        {
            return new BridgeResponse { Ok = false, Error = e.Message };
        }
    }
}

[SupportedOSPlatform("windows")]
public class FindAppHandler : IActionHandler
{
    public string Intent => "windows.findapp";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var query = payload.GetProperty("query").GetString() ?? "";
        var apps = AppResolver.Search(query, 8);
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { apps }) };
    }
}
