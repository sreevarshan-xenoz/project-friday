using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.InteropServices;
using WindowsHost.Model;

namespace WindowsHost.Win32;

/// <summary>
/// Maps the host's stable window ids (returned to Rust) to live HWNDs.
/// Rust never sees an HWND; it only ever holds these ids.
/// </summary>
public class WindowRegistry
{
    private readonly ConcurrentDictionary<string, IntPtr> _ids = new();
    private readonly ConcurrentDictionary<IntPtr, string> _hwnds = new();
    private int _counter;

    public string Register(IntPtr hwnd)
    {
        if (_hwnds.TryGetValue(hwnd, out var existing))
            return existing;
        var id = $"win-{Interlocked.Increment(ref _counter)}";
        _ids[id] = hwnd;
        _hwnds[hwnd] = id;
        return id;
    }

    public bool TryResolve(string id, out IntPtr hwnd) => _ids.TryGetValue(id, out hwnd);

    public void Forget(string id)
    {
        if (_ids.TryRemove(id, out var hwnd))
            _hwnds.TryRemove(hwnd, out _);
    }

    public Window ToWindow(IntPtr hwnd, string process, uint pid)
    {
        var id = Register(hwnd);
        var text = new System.Text.StringBuilder(Native.GetWindowTextLength(hwnd) + 1);
        Native.GetWindowText(hwnd, text, text.Capacity);
        Native.GetWindowRect(hwnd, out var r);
        var placement = new Native.WINDOWPLACEMENT { Length = Marshal.SizeOf<Native.WINDOWPLACEMENT>() };
        Native.GetWindowPlacement(hwnd, ref placement);
        var state = placement.ShowCmd switch
        {
            Native.SW_MINIMIZE => "Minimized",
            Native.SW_MAXIMIZE => "Maximized",
            Native.SW_HIDE => "Hidden",
            _ => "Normal",
        };
        return new Window
        {
            Id = id,
            Title = text.ToString(),
            Process = process,
            ProcessId = pid,
            Bounds = new Bounds
            {
                X = r.Left,
                Y = r.Top,
                Width = (uint)(r.Right - r.Left),
                Height = (uint)(r.Bottom - r.Top),
            },
            State = state,
            Monitor = 0,
        };
    }

    public static string ProcessName(uint pid)
    {
        try
        {
            using var p = Process.GetProcessById((int)pid);
            return p.ProcessName;
        }
        catch
        {
            return "unknown";
        }
    }
}
