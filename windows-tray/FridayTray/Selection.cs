using System.Runtime.InteropServices;
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
        try
        {
            var fg = Native.GetForegroundWindow();
            var className = Native.GetClassName(fg);
            if (Terminals.IsTerminalClass(className))
            {
                string? text = TryGetClipboardText();
                if (string.IsNullOrWhiteSpace(text))
                    return new Result(null, "", "Select or copy some text first, then press the hotkey again.");
                if (Asking.LooksLikeSecret(text) || MarkedPrivate())
                    return new Result(null, "", "The clipboard looks like a password or token, so I won't send it. Select the text you mean instead.");
                return new Result(text, "Clipboard", null);
            }

            await WaitForModifiersUpAsync();
            var savedClipboard = SnapshotClipboard();
            string? before = TryGetClipboardText();
            var beforePrivate = MarkedPrivate();
            var seq = Native.GetClipboardSequenceNumber();
            SendCtrlC();
            for (var i = 0; i < 20 && Native.GetClipboardSequenceNumber() == seq; i++)
                await Task.Delay(25);

            if (Native.GetClipboardSequenceNumber() != seq)
            {
                var selected = TryGetClipboardText();
                RestoreClipboard(savedClipboard);
                if (!string.IsNullOrWhiteSpace(selected))
                    return new Result(selected, "Selected", null);
            }

            if (string.IsNullOrWhiteSpace(before))
                return new Result(null, "", "Select or copy some text first, then press the hotkey again.");
            if (Asking.LooksLikeSecret(before) || beforePrivate)
                return new Result(null, "", "The clipboard looks like a password or token, so I won't send it. Select the text you mean instead.");
            return new Result(before, "Clipboard", null);
        }
        catch (ExternalException)
        {
            return new Result(null, "", "The clipboard is busy. Try again.");
        }
    }

    /// <summary>
    /// Pastes <paramref name="text"/> into <paramref name="target"/> with
    /// Ctrl+V (it must be in front again), then puts the clipboard back.
    /// False when the window couldn't be brought back.
    /// </summary>
    public static async Task<bool> PasteAsync(IntPtr target, string text)
    {
        Native.RestoreForeground(target);
        await FocusWait.WaitForForegroundAsync(target, Native.GetForegroundWindow);
        await WaitForModifiersUpAsync();
        if (Native.GetForegroundWindow() != target)
            return false;
        var saved = SnapshotClipboard();
        if (!TrySetText(text))
            return false;
        var ours = Native.GetClipboardSequenceNumber();
        try
        {
            // Checked again: the paste goes only to the window it was meant for.
            if (Native.GetForegroundWindow() != target)
                return false;
            SendChord(Native.VK_V);
            // The app reads the clipboard after the paste; put the old one back a moment later.
            await Task.Delay(1000);
            return true;
        }
        finally
        {
            // Unless something else was copied meanwhile, or the old one couldn't be read.
            if (saved != null && Native.GetClipboardSequenceNumber() == ours)
                RestoreClipboard(saved);
        }
    }

    /// <summary>Puts text on the clipboard; false when another program holds it.</summary>
    public static bool TrySetText(string text)
    {
        try
        {
            Clipboard.SetDataObject(text, copy: true, retryTimes: 10, retryDelay: 50);
            return true;
        }
        catch (ExternalException)
        {
            return false;
        }
    }

    /// <summary>
    /// Password managers mark what they copy so clipboard history and other
    /// tools leave it alone; FRIDAY does too.
    /// </summary>
    private static bool MarkedPrivate()
    {
        try
        {
            return Clipboard.ContainsData("ExcludeClipboardContentFromMonitorProcessing")
                || Clipboard.ContainsData("Clipboard Viewer Ignore");
        }
        catch (ExternalException)
        {
            return false;
        }
    }

    private static string? TryGetClipboardText()
    {
        for (var attempt = 0; attempt < 10; attempt++)
        {
            try
            {
                return Clipboard.ContainsText() ? Clipboard.GetText() : null;
            }
            catch (ExternalException)
            {
                if (attempt == 9)
                    throw;
                Thread.Sleep(50);
            }
        }
        return null;
    }

    private static IDataObject? SnapshotClipboard()
    {
        try
        {
            var dataObj = Clipboard.GetDataObject();
            if (dataObj == null)
                return null;

            var formats = dataObj.GetFormats();
            if (formats == null || formats.Length == 0)
                return null;

            var copy = new DataObject();
            foreach (var format in formats)
            {
                try
                {
                    var data = dataObj.GetData(format);
                    if (data != null)
                        copy.SetData(format, data);
                }
                catch
                {
                    // Ignore formats that cannot be retrieved or cloned
                }
            }
            return copy.GetFormats().Length > 0 ? copy : dataObj;
        }
        catch
        {
            return null;
        }
    }

    private static void RestoreClipboard(IDataObject? saved)
    {
        try
        {
            if (saved != null)
                Clipboard.SetDataObject(saved, copy: true, retryTimes: 10, retryDelay: 50);
            else
                Clipboard.Clear();
        }
        catch
        {
            // Best effort restoration
        }
    }

    private static async Task WaitForModifiersUpAsync()
    {
        int[] keys = [Native.VK_LWIN, Native.VK_RWIN, Native.VK_MENU, Native.VK_CONTROL, Native.VK_SHIFT];
        for (var i = 0; i < 40 && keys.Any(k => (Native.GetAsyncKeyState(k) & 0x8000) != 0); i++)
            await Task.Delay(25);
    }

    private static void SendCtrlC() => SendChord(Native.VK_C);

    private static void SendChord(int vk)
    {
        static Native.INPUT Key(int vk, bool up) => new()
        {
            type = Native.INPUT_KEYBOARD,
            ki = new Native.KEYBDINPUT { wVk = (ushort)vk, dwFlags = up ? Native.KEYEVENTF_KEYUP : 0 },
        };
        Native.INPUT[] inputs =
        [
            Key(Native.VK_CONTROL, false), Key(vk, false),
            Key(vk, true), Key(Native.VK_CONTROL, true),
        ];
        Native.SendInput((uint)inputs.Length, inputs, System.Runtime.InteropServices.Marshal.SizeOf<Native.INPUT>());
    }
}
