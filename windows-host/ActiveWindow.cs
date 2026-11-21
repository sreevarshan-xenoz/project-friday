using System.Drawing;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Text.Json;

namespace WindowsHost;

/// <summary>
/// `friday-windows-host --active-window`: print the focused window as JSON,
/// in the shape of Hyprland's `hyprctl -j activewindow`
/// (<c>{"class": .., "title": .., "at": [x, y], "size": [w, h]}</c>), and exit
/// 0; exit 1 when there is no window to speak of (the desktop, the taskbar, a
/// minimized window). The daemon uses it to capture just the window a screen
/// ask is about, and falls back to the whole screen on exit 1. Coordinates are
/// physical desktop pixels, the same as `--screenshot x y w h` takes.
/// </summary>
[SupportedOSPlatform("windows")]
public static class ActiveWindow
{
    private static readonly IntPtr DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = new(-4);
    private const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    private const int SM_XVIRTUALSCREEN = 76, SM_YVIRTUALSCREEN = 77, SM_CXVIRTUALSCREEN = 78, SM_CYVIRTUALSCREEN = 79;

    // Not windows anyone asks about: the desktop and the taskbars.
    private static readonly string[] Shell = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("user32.dll")] private static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("dwmapi.dll")] private static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    public static int Run()
    {
        try
        {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            var h = GetForegroundWindow();
            if (h == IntPtr.Zero || IsIconic(h))
                return 1;
            var cls = Text(h, GetClassName);
            if (Shell.Contains(cls))
                return 1;

            // The visible frame (without the drop shadow GetWindowRect includes).
            if (DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, out var r, Marshal.SizeOf<RECT>()) != 0
                && !GetWindowRect(h, out r))
                return 1;
            var desktop = new Rectangle(
                GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            var bounds = Rectangle.Intersect(Rectangle.FromLTRB(r.Left, r.Top, r.Right, r.Bottom), desktop);
            if (bounds.Width <= 0 || bounds.Height <= 0)
                return 1;

            var json = JsonSerializer.Serialize(new Dictionary<string, object>
            {
                // The app, as Hyprland's class is: its process name
                // ("notepad", "chrome"), which is how the window-text reader
                // and the private-window list name apps. The Win32 class too.
                ["class"] = ProcessName(h) ?? cls,
                ["win32_class"] = cls,
                ["title"] = Text(h, GetWindowText),
                ["at"] = new[] { bounds.X, bounds.Y },
                ["size"] = new[] { bounds.Width, bounds.Height },
            });
            Console.Out.Write(json);
            return 0;
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"active window: {e.Message}");
            return 1;
        }
    }

    private static string? ProcessName(IntPtr h)
    {
        try
        {
            GetWindowThreadProcessId(h, out var pid);
            using var p = System.Diagnostics.Process.GetProcessById((int)pid);
            return p.ProcessName.ToLowerInvariant();
        }
        catch (Exception)
        {
            return null;
        }
    }

    private static string Text(IntPtr h, Func<IntPtr, StringBuilder, int, int> get)
    {
        var sb = new StringBuilder(512);
        get(h, sb, sb.Capacity);
        return sb.ToString();
    }
}
