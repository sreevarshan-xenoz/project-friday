using System.Collections.Generic;
using System.Diagnostics;
using System.ComponentModel;
using System.IO;
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
        var targetName = Path.GetFileNameWithoutExtension(path).ToLowerInvariant();

        // Snapshot windows matching the target BEFORE launch. This lets us tell
        // a newly-created instance from an already-running one that is simply
        // activated: single-instance apps (modern Notepad, Edge, Settings,
        // Explorer, ...) hand the launch request to an existing instance instead
        // of spawning a new window, so the launched pid never owns a window.
        var before = MatchingWindows(targetName);

        var start = new ProcessStartInfo(path, args ?? "")
        {
            // UseShellExecute = false keeps the child on the host's own window
            // station/desktop so its window is enumerable here.
            UseShellExecute = false,
            WindowStyle = ProcessWindowStyle.Normal,
        };
        using var process = Process.Start(start)
            ?? throw new Win32Exception(2, "failed to start process");

        var (hwnd, resolution) = AcquireWindow(process.Id, targetName, before);
        if (hwnd == IntPtr.Zero)
            throw new Win32Exception(2, "process started but no window");

        // Resolve the real owning process for the window we return — for an
        // existing instance this differs from the launched pid.
        Native.GetWindowThreadProcessId(hwnd, out var ownerPid);
        var win = registry.ToWindow(hwnd, WindowRegistry.ProcessName(ownerPid), ownerPid);
        return new BridgeResponse
        {
            Ok = true,
            Output = Serde.ToElement(win),
            Meta = new Dictionary<string, string> { ["launch_resolution"] = resolution },
        };
    }

    /// <summary>
    /// Reason about the launch outcome as a capability ("acquire a usable window
    /// for this request") rather than "spawn a process". Returns the window and
    /// whether a new instance was created or an existing one was activated. Both
    /// outcomes are success.
    /// </summary>
    private static (IntPtr hwnd, string resolution) AcquireWindow(
        int launchedPid, string targetName, HashSet<IntPtr> before)
    {
        // 1) New instance: a window owned by the launched pid appears.
        var owned = WaitForOwnedWindow(launchedPid);
        if (owned != IntPtr.Zero)
            return (owned, "New Instance");

        // 2) A fresh window matching the target appeared under a different pid
        //    (single-instance broker that re-parents the request).
        var after = MatchingWindows(targetName);
        foreach (var h in after)
        {
            if (!before.Contains(h)) return (h, "New Instance");
        }

        // 3) Existing instance was activated: the foreground window (or any
        //    pre-existing matching window) now represents the request.
        Thread.Sleep(300);
        var fg = Native.GetForegroundWindow();
        if (WindowProcessName(fg) == targetName)
            return (fg, "Existing Instance");
        foreach (var h in before)
            return (h, "Existing Instance");

        return (IntPtr.Zero, "Failed");
    }

    private static IntPtr WaitForOwnedWindow(int pid)
    {
        for (var i = 0; i < 100; i++)
        {
            var hwnd = FirstVisibleWindowOf(pid);
            if (hwnd != IntPtr.Zero) return hwnd;
            Thread.Sleep(80);
        }
        return IntPtr.Zero;
    }

    private static IntPtr FirstVisibleWindowOf(int pid)
    {
        IntPtr found = IntPtr.Zero;
        Native.EnumWindows((h, _) =>
        {
            Native.GetWindowThreadProcessId(h, out var p);
            if ((int)p == pid)
            {
                // Prefer a visible, owner-less top-level window (the main
                // window) but accept any window that belongs to the pid so we
                // don't miss processes whose main window has an owner.
                if (found == IntPtr.Zero) found = h;
                if (Native.IsWindowVisible(h) && Native.GetWindow(h, Native.GW_OWNER) == IntPtr.Zero)
                {
                    found = h;
                    return false;
                }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    private static HashSet<IntPtr> MatchingWindows(string targetName)
    {
        var set = new HashSet<IntPtr>();
        Native.EnumWindows((h, _) =>
        {
            if (Native.IsWindowVisible(h) && WindowProcessName(h) == targetName)
                set.Add(h);
            return true;
        }, IntPtr.Zero);
        return set;
    }

    private static string WindowProcessName(IntPtr hwnd)
    {
        Native.GetWindowThreadProcessId(hwnd, out var pid);
        return WindowRegistry.ProcessName(pid).ToLowerInvariant();
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { closed = id }) };
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(new { focused = id }) };
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(win) };
    }

    public static (IntPtr, Window) Resolve(JsonElement payload, WindowRegistry registry)
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(win) };
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(windows) };
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
        return new BridgeResponse { Ok = true, Output = Serde.ToElement(win) };
    }
}
