using System.ComponentModel;
using System.Text.Json;
using WindowsHost.IPC;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

/// <summary>
/// Routes an intent (e.g. "windows.open") to its handler. Throws only
/// BridgeError-coded responses — never raw Win32 exceptions.
/// </summary>
public class Dispatcher
{
    private readonly Dictionary<string, IActionHandler> _handlers = new();

    public Dispatcher()
    {
        Register(new OpenApplicationHandler());
        Register(new CloseApplicationHandler());
        Register(new FocusWindowHandler());
        Register(new MoveWindowHandler());
        Register(new ResizeWindowHandler());
        Register(new CaptureWindowHandler());
        Register(new ListWindowsHandler());
        Register(new ForegroundWindowHandler());
        Register(new ClickHandler());
        Register(new TypeHandler());
        Register(new ScrollHandler());
        Register(new ClipboardReadHandler());
        Register(new ClipboardWriteHandler());
        Register(new MediaHandler());
        Register(new NowPlayingHandler());
        Register(new VolumeHandler());
        Register(new BrightnessHandler());
        Register(new DesktopsHandler());
        Register(new FindAppHandler());
        Register(new HealthHandler());
    }

    private void Register(IActionHandler handler)
    {
        // The handler's intent is derived from its type name by convention;
        // each handler also hard-codes its verb via Intent.
        _handlers[handler.Intent] = handler;
    }

    public BridgeResponse Dispatch(string intent, JsonElement payload, WindowRegistry registry)
    {
        if (_handlers.TryGetValue(intent, out var handler))
        {
            try
            {
                return handler.Handle(payload, registry);
            }
            catch (Win32Exception ex)
            {
                return ErrorFor(ex);
            }
            catch (Exception ex)
            {
                return new BridgeResponse { Ok = false, Error = $"Unknown:{ex.Message}" };
            }
        }
        return new BridgeResponse { Ok = false, Error = $"Unknown:no handler for {intent}" };
    }

    private static BridgeResponse ErrorFor(Win32Exception ex)
    {
        var code = ex.NativeErrorCode switch
        {
            1400 => "InvalidHandle",   // ERROR_INVALID_WINDOW_HANDLE
            1409 => "InvalidHandle",   // ERROR_INVALID_CURSOR_HANDLE
            5 => "AccessDenied",       // ERROR_ACCESS_DENIED
            2 => "WindowNotFound",     // ERROR_FILE_NOT_FOUND
            1460 => "Timeout",         // ERROR_TIMEOUT
            _ => $"Unknown:win32-{ex.NativeErrorCode}",
        };
        return new BridgeResponse { Ok = false, Error = code };
    }
}
