using System.Runtime.InteropServices;

namespace FridayTray;

/// <summary>The Win32 calls the tray needs: hotkeys, synthetic Ctrl+C, job objects.</summary>
internal static partial class Native
{
    public const int WM_HOTKEY = 0x0312;
    public const uint MOD_ALT = 0x1, MOD_CONTROL = 0x2, MOD_SHIFT = 0x4, MOD_WIN = 0x8, MOD_NOREPEAT = 0x4000;
    public const int VK_SHIFT = 0x10, VK_CONTROL = 0x11, VK_MENU = 0x12, VK_LWIN = 0x5B, VK_RWIN = 0x5C, VK_C = 0x43;

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool UnregisterHotKey(IntPtr hWnd, int id);

    [LibraryImport("user32.dll")]
    public static partial short GetAsyncKeyState(int vKey);

    [LibraryImport("user32.dll")]
    public static partial uint GetClipboardSequenceNumber();

    [LibraryImport("user32.dll")]
    public static partial IntPtr GetForegroundWindow();

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetForegroundWindow(IntPtr hWnd);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool AllowSetForegroundWindow(uint dwProcessId);

    public const uint ASFW_ANY = unchecked((uint)-1);

    [LibraryImport("user32.dll", SetLastError = true)]
    public static partial uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [LibraryImport("kernel32.dll")]
    public static partial uint GetCurrentThreadId();

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool AttachThreadInput(uint idAttach, uint idAttachTo, [MarshalAs(UnmanagedType.Bool)] bool fAttach);

    [LibraryImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool BringWindowToTop(IntPtr hWnd);

    [LibraryImport("user32.dll", EntryPoint = "GetClassNameW", SetLastError = true)]
    public static unsafe partial int GetClassName(IntPtr hWnd, char* lpClassName, int nMaxCount);

    public static unsafe string GetClassName(IntPtr hWnd)
    {
        if (hWnd == IntPtr.Zero)
            return string.Empty;
        const int maxCount = 256;
        char* buf = stackalloc char[maxCount];
        int len = GetClassName(hWnd, buf, maxCount);
        return len > 0 ? new string(buf, 0, len) : string.Empty;
    }

    /// <summary>
    /// Restores focus to the specified window, using AllowSetForegroundWindow and
    /// AttachThreadInput workarounds if standard SetForegroundWindow is restricted.
    /// </summary>
    public static bool RestoreForeground(IntPtr hWnd)
    {
        if (hWnd == IntPtr.Zero)
            return false;

        if (GetForegroundWindow() == hWnd)
            return true;

        uint currentThreadId = GetCurrentThreadId();
        IntPtr fg = GetForegroundWindow();
        uint fgThreadId = fg != IntPtr.Zero ? GetWindowThreadProcessId(fg, out _) : 0;
        uint targetThreadId = GetWindowThreadProcessId(hWnd, out uint targetProcId);

        if (targetProcId != 0)
            AllowSetForegroundWindow(targetProcId);
        else
            AllowSetForegroundWindow(ASFW_ANY);

        if (SetForegroundWindow(hWnd))
            return true;

        bool attachedFg = false;
        bool attachedTarget = false;
        try
        {
            if (fgThreadId != 0 && fgThreadId != currentThreadId)
                attachedFg = AttachThreadInput(currentThreadId, fgThreadId, true);
            if (targetThreadId != 0 && targetThreadId != currentThreadId && targetThreadId != fgThreadId)
                attachedTarget = AttachThreadInput(currentThreadId, targetThreadId, true);

            SetForegroundWindow(hWnd);
            BringWindowToTop(hWnd);
        }
        finally
        {
            if (attachedTarget)
                AttachThreadInput(currentThreadId, targetThreadId, false);
            if (attachedFg)
                AttachThreadInput(currentThreadId, fgThreadId, false);
        }

        return GetForegroundWindow() == hWnd;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT
    {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    // INPUT is a union; the keyboard member is the only one used, padded to
    // the size of the largest (MOUSEINPUT) so SendInput accepts cbSize.
    [StructLayout(LayoutKind.Explicit, Size = 40)]
    public struct INPUT
    {
        [FieldOffset(0)] public uint type;
        [FieldOffset(8)] public KEYBDINPUT ki;
    }

    public const uint INPUT_KEYBOARD = 1, KEYEVENTF_KEYUP = 0x2;

    [LibraryImport("user32.dll", SetLastError = true)]
    public static partial uint SendInput(uint nInputs, [In] INPUT[] pInputs, int cbSize);

    // Job object: children assigned to it die when the tray exits, even if it
    // crashes, so a restart never finds an orphaned daemon holding the port.
    [LibraryImport("kernel32.dll", SetLastError = true, StringMarshalling = StringMarshalling.Utf16)]
    public static partial IntPtr CreateJobObjectW(IntPtr lpJobAttributes, string? lpName);

    [LibraryImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetInformationJobObject(IntPtr hJob, int infoClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION info, int cbInfo);

    [LibraryImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool AssignProcessToJobObject(IntPtr hJob, IntPtr hProcess);

    public const int JobObjectExtendedLimitInformation = 9;
    public const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;

    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_BASIC_LIMIT_INFORMATION
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct IO_COUNTERS
    {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    {
        public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }
}
