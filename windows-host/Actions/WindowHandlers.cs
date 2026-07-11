using System.Diagnostics;
using System.Text.Json;
using WindowsHost.IPC;
using WindowsHost.Model;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

public class OpenApplicationHandler : IActionHandler
{
    public string Intent => "windows.open";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var path = payload.GetProperty("path").GetString()
            ?? throw new ArgumentException("missing path");
        var args = payload.TryGetProperty("args", out var a) ? a.GetString() : null;

        var start = new ProcessStartInfo(path, args ?? "")
        {
            UseShellExecute = true,
            WindowStyle = ProcessWindowStyle.Normal,
        };
        using var process = Process.Start(start)
            ?? throw new Win32Exception(2, "failed to start process");

        // Wait briefly for the main window handle to appear.
        var hwnd = IntPtr.Zero;
        for (var i = 0; i < 20; i++)
        {
            hwnd = process.MainWindowHandle;
            if (hwnd != IntPtr.Zero) break;
            Thread.Sleep(100);
        }
        if (hwnd == IntPtr.Zero)
        {
            // Fall back to enumerating by process id.
            hwnd = FirstVisibleWindowOf(process.Id);
        }
        if (hwnd == IntPtr.Zero)
            throw new Win32Exception(2, "process started but no window");

        var win = registry.ToWindow(hwnd, process.ProcessName, (uint)process.Id);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(win) };
    }

    private static IntPtr FirstVisibleWindowOf(int pid)
    {
        IntPtr found = IntPtr.Zero;
        Native.EnumWindows((h, _) =>
        {
            Native.GetWindowThreadProcessId(h, out var p);
            if ((int)p == pid && Native.IsWindowVisible(h) && Native.GetWindow(h, Native.GW_OWNER) == IntPtr.Zero)
            {
                found = h;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}

public class CloseApplicationHandler : IActionHandler
{
    public string Intent => "windows.close";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var id = payload.GetProperty("window_id").GetString()
            ?? throw new ArgumentException("missing window_id");
        if (!registry.TryResolve(id, out var hwnd))
            return new BridgeResponse { Ok = false, Error = "WindowNotFound" };

        Native.PostMessage(hwnd, Native.WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
        registry.Forget(id);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { closed = id }) };
    }
}

public class FocusWindowHandler : IActionHandler
{
    public string Intent => "windows.focus";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var id = payload.GetProperty("window_id").GetString()
            ?? throw new ArgumentException("missing window_id");
        if (!registry.TryResolve(id, out var hwnd))
            return new BridgeResponse { Ok = false, Error = "WindowNotFound" };

        Native.ShowWindow(hwnd, Native.SW_RESTORE);
        Native.SetForegroundWindow(hwnd);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { focused = id }) };
    }
}

public class MoveWindowHandler : IActionHandler
{
    public string Intent => "windows.move";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, win) = Resolve(payload, registry);
        var x = payload.GetProperty("x").GetInt32();
        var y = payload.GetProperty("y").GetInt32();
        Native.MoveWindow(hwnd, x, y, (int)win.Bounds.Width, (int)win.Bounds.Height, true);
        win.Bounds.X = x;
        win.Bounds.Y = y;
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(win) };
    }

    protected static (IntPtr, Window) Resolve(JsonElement payload, WindowRegistry registry)
    {
        var id = payload.GetProperty("window_id").GetString()
            ?? throw new ArgumentException("missing window_id");
        if (!registry.TryResolve(id, out var hwnd))
            throw new Win32Exception(1400, "window not found");
        return (hwnd, registry.ToWindow(hwnd, "", 0));
    }
}

public class ResizeWindowHandler : IActionHandler
{
    public string Intent => "windows.resize";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, win) = MoveWindowHandler.Resolve(payload, registry);
        var w = payload.GetProperty("width").GetInt32();
        var h = payload.GetProperty("height").GetInt32();
        Native.MoveWindow(hwnd, win.Bounds.X, win.Bounds.Y, w, h, true);
        win.Bounds.Width = (uint)w;
        win.Bounds.Height = (uint)h;
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(win) };
    }
}

public class ListWindowsHandler : IActionHandler
{
    public string Intent => "windows.list";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var windows = new List<Window>();
        Native.EnumWindows((h, _) =>
        {
            if (Native.IsWindowVisible(h) && Native.GetWindow(h, Native.GW_OWNER) == IntPtr.Zero)
            {
                Native.GetWindowThreadProcessId(h, out var pid);
                windows.Add(registry.ToWindow(h, WindowRegistry.ProcessName(pid), pid));
            }
            return true;
        }, IntPtr.Zero);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(windows) };
    }
}

public class ForegroundWindowHandler : IActionHandler
{
    public string Intent => "windows.foreground";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var hwnd = Native.GetForegroundWindow();
        if (hwnd == IntPtr.Zero)
            return new BridgeResponse { Ok = false, Error = "WindowNotFound" };
        Native.GetWindowThreadProcessId(hwnd, out var pid);
        var win = registry.ToWindow(hwnd, WindowRegistry.ProcessName(pid), pid);
        return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(win) };
    }
}
