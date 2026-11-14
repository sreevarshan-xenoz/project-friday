using System.Runtime.InteropServices;
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { ok = true }) };
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

        var inputs = BuildInputs(text);
        if (inputs.Length > 0)
        {
            Native.SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Native.INPUT>());
        }
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { ok = true }) };
    }

    public static Native.INPUT[] BuildInputs(string text)
    {
        var inputs = new Native.INPUT[text.Length * 2];
        for (var i = 0; i < text.Length; i++)
        {
            var c = text[i];
            inputs[2 * i] = new Native.INPUT
            {
                type = Native.INPUT_KEYBOARD,
                ki = new Native.KEYBDINPUT
                {
                    wVk = 0,
                    wScan = c,
                    dwFlags = Native.KEYEVENTF_UNICODE,
                },
            };
            inputs[2 * i + 1] = new Native.INPUT
            {
                type = Native.INPUT_KEYBOARD,
                ki = new Native.KEYBDINPUT
                {
                    wVk = 0,
                    wScan = c,
                    dwFlags = Native.KEYEVENTF_UNICODE | Native.KEYEVENTF_KEYUP,
                },
            };
        }
        return inputs;
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { ok = true }) };
    }
}
