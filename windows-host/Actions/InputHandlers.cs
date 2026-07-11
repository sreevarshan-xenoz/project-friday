using System.Text.Json;
using WindowsHost.IPC;
using WindowsHost.Model;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

/// <summary>
/// Vision-ready input intents. Each carries a Window id + coordinates so the
/// future Vision module can supply targets without the bridge changing.
/// </summary>
public class ClickHandler : IActionHandler
{
    public string Intent => "windows.click";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, win) = MoveWindowHandler.Resolve(payload, registry);
        var x = payload.GetProperty("x").GetInt32();
        var y = payload.GetProperty("y").GetInt32();

        Native.ShowWindow(hwnd, Native.SW_RESTORE);
        Native.SetForegroundWindow(hwnd);
        var absX = win.Bounds.X + x;
        var absY = win.Bounds.Y + y;
        Native.SetCursorPos(absX, absY);
        Native.mouse_event(Native.MOUSEEVENTF_LEFTDOWN, absX, absY, 0, UIntPtr.Zero);
        Native.mouse_event(Native.MOUSEEVENTF_LEFTUP, absX, absY, 0, UIntPtr.Zero);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { ok = true }) };
    }
}

public class TypeHandler : IActionHandler
{
    public string Intent => "windows.type";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, _) = MoveWindowHandler.Resolve(payload, registry);
        var text = payload.GetProperty("text").GetString()
            ?? throw new ArgumentException("missing text");

        Native.ShowWindow(hwnd, Native.SW_RESTORE);
        Native.SetForegroundWindow(hwnd);
        Thread.Sleep(50);

        // Send each character through the active keyboard layout.
        foreach (var c in text)
        {
            Native.keybd_event((byte)c, 0, 0, UIntPtr.Zero);
            Native.keybd_event((byte)c, 0, Native.KEYEVENTF_KEYUP, UIntPtr.Zero);
        }
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { ok = true }) };
    }
}

public class ScrollHandler : IActionHandler
{
    public string Intent => "windows.scroll";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, win) = MoveWindowHandler.Resolve(payload, registry);
        var delta = payload.GetProperty("delta").GetInt32();

        Native.ShowWindow(hwnd, Native.SW_RESTORE);
        Native.SetForegroundWindow(hwnd);
        // WM_MOUSEWHEEL wParam high-order word carries the scroll delta.
        var wParam = (IntPtr)((delta << 16) & 0xFFFF0000);
        Native.PostMessage(hwnd, Native.WM_MOUSEWHEEL, wParam, IntPtr.Zero);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { ok = true }) };
    }
}
