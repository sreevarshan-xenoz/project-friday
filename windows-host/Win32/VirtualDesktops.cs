using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Diagnostics;
using Microsoft.Win32;

namespace WindowsHost.Win32;

/// <summary>
/// Virtual desktops. Windows has no public API for them, so the count and the
/// current one are read from the registry Explorer keeps them in (stable since
/// Windows 10), and switching presses Win+Ctrl+Left/Right, the shortcut every
/// version has. Moving another app's window between desktops has no stable
/// route, so it isn't offered.
/// </summary>
[SupportedOSPlatform("windows")]
public static class VirtualDesktops
{
    private const string Base = @"Software\Microsoft\Windows\CurrentVersion\Explorer";
    private const ushort VkLwin = 0x5B, VkControl = 0x11, VkLeft = 0x25, VkRight = 0x27;

    /// <summary>The number of desktops and the current one (both 1-based).</summary>
    public static (int Count, int Current) Read()
    {
        using var root = Registry.CurrentUser;
        var ids = (root.OpenSubKey($@"{Base}\VirtualDesktops")?.GetValue("VirtualDesktopIDs")) as byte[];
        var count = Math.Max(1, (ids?.Length ?? 0) / 16);
        // Windows 11 keeps the current desktop per session; Windows 10 in one place.
        var session = Process.GetCurrentProcess().SessionId;
        var current = (root.OpenSubKey($@"{Base}\SessionInfo\{session}\VirtualDesktops")?.GetValue("CurrentVirtualDesktop") as byte[])
            ?? (root.OpenSubKey($@"{Base}\VirtualDesktops")?.GetValue("CurrentVirtualDesktop") as byte[]);
        if (ids is null || current is null || current.Length != 16)
            return (count, 1);
        for (var i = 0; i < count; i++)
            if (ids.AsSpan(i * 16, 16).SequenceEqual(current))
                return (count, i + 1);
        return (count, 1);
    }

    /// <summary>
    /// Goes to desktop <paramref name="to"/> (1-based), or by <paramref name="by"/>
    /// desktops from the current one when <paramref name="to"/> is null, and
    /// returns where Windows ended up. Never creates a desktop.
    /// </summary>
    public static (int Count, int Current) Go(int? to, int by)
    {
        var (count, current) = Read();
        var target = to ?? current + by;
        if (target < 1 || target > count)
            throw new ArgumentException(count == 1
                ? "There is only one desktop"
                : $"There are {count} desktops (you are on {current}); there is no desktop {target}");
        for (var i = 0; i < Math.Abs(target - current); i++)
        {
            Chord(target > current ? VkRight : VkLeft);
            Thread.Sleep(200);
        }
        // The switch is animated; wait for the registry to follow.
        for (var i = 0; i < 10 && Read().Current != target && target != current; i++)
            Thread.Sleep(100);
        return Read();
    }

    private static void Chord(ushort arrow)
    {
        ushort[] keys = [VkLwin, VkControl, arrow];
        var inputs = new Native.INPUT[keys.Length * 2];
        for (var i = 0; i < keys.Length; i++)
        {
            inputs[i] = Key(keys[i], 0);
            inputs[inputs.Length - 1 - i] = Key(keys[i], Native.KEYEVENTF_KEYUP);
        }
        Native.SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Native.INPUT>());
    }

    private static Native.INPUT Key(ushort vk, uint flags) => new()
    {
        type = Native.INPUT_KEYBOARD,
        ki = new Native.KEYBDINPUT { wVk = vk, dwFlags = flags },
    };
}
