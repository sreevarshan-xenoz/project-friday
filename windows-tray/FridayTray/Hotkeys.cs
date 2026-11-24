using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// System-wide hotkeys. Each action tries the Windows key first (the same
/// keys as Super on Linux) and falls back to Ctrl+Alt if another program
/// already owns it. <see cref="Labels"/> says which ones were actually taken.
/// </summary>
internal sealed class Hotkeys : NativeWindow, IDisposable
{
    private readonly Dictionary<int, AskKind> _ids = new();
    private readonly Action<TimeSpan>? _delay;
    public Dictionary<AskKind, string> Labels { get; } = new();

    public event Action<AskKind>? Pressed;

    /// <summary>The clipboard changed (once <see cref="WatchClipboard"/> is on).</summary>
    public event Action? ClipboardChanged;

    public void WatchClipboard() => Native.AddClipboardFormatListener(Handle);

    public Hotkeys(Action<TimeSpan>? delay = null)
    {
        _delay = delay;
        CreateHandle(new CreateParams());
    }

    public void RegisterAll()
    {
        Register(1, AskKind.Plain,
            (Native.MOD_WIN, 'Y', "Win+Y"), (Native.MOD_CONTROL | Native.MOD_ALT, 'Y', "Ctrl+Alt+Y"));
        Register(2, AskKind.Selection,
            (Native.MOD_WIN | Native.MOD_ALT, 'Y', "Win+Alt+Y"),
            (Native.MOD_CONTROL | Native.MOD_ALT | Native.MOD_SHIFT, 'Y', "Ctrl+Alt+Shift+Y"));
        Register(3, AskKind.Screen,
            (Native.MOD_WIN | Native.MOD_CONTROL, 'Y', "Win+Ctrl+Y"),
            (Native.MOD_CONTROL | Native.MOD_ALT | Native.MOD_SHIFT, 'S', "Ctrl+Alt+Shift+S"));
    }

    private void Register(int id, AskKind kind, params (uint mods, char key, string label)[] choices)
    {
        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            choices,
            c => Native.RegisterHotKey(Handle, id, c.mods | Native.MOD_NOREPEAT, c.key),
            delay: _delay);

        if (ok)
        {
            _ids[id] = kind;
            Labels[kind] = chosen.label;
        }
    }

    protected override void WndProc(ref Message m)
    {
        if (m.Msg == Native.WM_HOTKEY && _ids.TryGetValue((int)m.WParam, out var kind))
            Pressed?.Invoke(kind);
        else if (m.Msg == Native.WM_CLIPBOARDUPDATE)
            ClipboardChanged?.Invoke();
        base.WndProc(ref m);
    }

    public void Dispose()
    {
        foreach (var id in _ids.Keys)
            Native.UnregisterHotKey(Handle, id);
        DestroyHandle();
    }
}
