using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// The text to ask about. Windows has no "primary selection", so: wait for
/// the hotkey's modifiers to come up, send Ctrl+C to the focused window, and
/// if the clipboard changes that is the selection (the previous clipboard
/// text is put back). Otherwise fall back to what was already copied, unless
/// it looks like a password or token.
/// </summary>
internal static class Selection
{
    public sealed record Result(string? Text, string Source, string? Problem);

    public static async Task<Result> CaptureAsync()
    {
        await WaitForModifiersUpAsync();
        string? before = Clipboard.ContainsText() ? Clipboard.GetText() : null;
        var seq = Native.GetClipboardSequenceNumber();
        SendCtrlC();
        for (var i = 0; i < 20 && Native.GetClipboardSequenceNumber() == seq; i++)
            await Task.Delay(25);

        if (Native.GetClipboardSequenceNumber() != seq && Clipboard.ContainsText())
        {
            var selected = Clipboard.GetText();
            if (before != null)
                Clipboard.SetText(before);
            if (!string.IsNullOrWhiteSpace(selected))
                return new Result(selected, "Selected", null);
        }

        if (string.IsNullOrWhiteSpace(before))
            return new Result(null, "", "Select or copy some text first, then press the hotkey again.");
        if (Asking.LooksLikeSecret(before))
            return new Result(null, "", "The clipboard looks like a password or token, so I won't send it. Select the text you mean instead.");
        return new Result(before, "Clipboard", null);
    }

    private static async Task WaitForModifiersUpAsync()
    {
        int[] keys = [Native.VK_LWIN, Native.VK_RWIN, Native.VK_MENU, Native.VK_CONTROL, Native.VK_SHIFT];
        for (var i = 0; i < 40 && keys.Any(k => (Native.GetAsyncKeyState(k) & 0x8000) != 0); i++)
            await Task.Delay(25);
    }

    private static void SendCtrlC()
    {
        static Native.INPUT Key(int vk, bool up) => new()
        {
            type = Native.INPUT_KEYBOARD,
            ki = new Native.KEYBDINPUT { wVk = (ushort)vk, dwFlags = up ? Native.KEYEVENTF_KEYUP : 0 },
        };
        Native.INPUT[] inputs =
        [
            Key(Native.VK_CONTROL, false), Key(Native.VK_C, false),
            Key(Native.VK_C, true), Key(Native.VK_CONTROL, true),
        ];
        Native.SendInput((uint)inputs.Length, inputs, System.Runtime.InteropServices.Marshal.SizeOf<Native.INPUT>());
    }
}
