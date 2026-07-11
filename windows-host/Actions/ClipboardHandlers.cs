using System.Text.Json;
using System.Text;
using WindowsHost.IPC;
using WindowsHost.Win32;

namespace WindowsHost.Actions;

public class ClipboardReadHandler : IActionHandler
{
    public string Intent => "windows.clipboard.read";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        if (!Native.OpenClipboard(IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "open clipboard failed");
        try
        {
            var h = Native.GetClipboardData(Native.CF_UNICODETEXT);
            if (h == IntPtr.Zero)
                return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { text = "" }) };
            var ptr = Native.GlobalLock(h);
            var text = Marshal.PtrToStringUni(ptr) ?? "";
            Native.GlobalUnlock(h);
            return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { text }) };
        }
        finally
        {
            Native.CloseClipboard();
        }
    }
}

public class ClipboardWriteHandler : IActionHandler
{
    public string Intent => "windows.clipboard.write";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var text = payload.GetProperty("text").GetString()
            ?? throw new ArgumentException("missing text");
        var bytes = Encoding.Unicode.GetBytes(text + "\0");
        var h = Native.GlobalAlloc(Native.GMEM_MOVEABLE, (uint)bytes.Length);
        var ptr = Native.GlobalLock(h);
        Marshal.Copy(bytes, 0, ptr, bytes.Length);
        Native.GlobalUnlock(h);

        if (!Native.OpenClipboard(IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "open clipboard failed");
        try
        {
            Native.EmptyClipboard();
            if (Native.SetClipboardData(Native.CF_UNICODETEXT, h) == IntPtr.Zero)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "set clipboard failed");
            return new BridgeResponse { Ok = true, Output = JsonSerializer.SerializeToElement(new { ok = true }) };
        }
        finally
        {
            Native.CloseClipboard();
        }
    }
}

public class CaptureWindowHandler : IActionHandler
{
    public string Intent => "windows.capture";

    public BridgeResponse Handle(JsonElement payload, WindowRegistry registry)
    {
        var (hwnd, win) = MoveWindowHandler.Resolve(payload, registry);
        var w = (int)win.Bounds.Width;
        var h = (int)win.Bounds.Height;
        if (w <= 0 || h <= 0)
            throw new Win32Exception(1400, "invalid window size");

        var screen = Native.GetDC(hwnd);
        var mem = Native.CreateCompatibleDC(screen);
        var bmp = Native.CreateCompatibleBitmap(screen, w, h);
        var old = Native.SelectObject(mem, bmp);
        Native.BitBlt(mem, 0, 0, w, h, screen, 0, 0, Native.SRCCOPY);

        using var bitmap = System.Drawing.Image.FromHbitmap(bmp);
        using var ms = new MemoryStream();
        bitmap.Save(ms, System.Drawing.Imaging.ImageFormat.Png);
        var base64 = Convert.ToBase64String(ms.ToArray());

        Native.SelectObject(mem, old);
        Native.DeleteObject(bmp);
        Native.DeleteDC(mem);
        Native.ReleaseDC(hwnd, screen);

        return new BridgeResponse
        {
            Ok = true,
            Output = JsonSerializer.SerializeToElement(new { data = base64, width = w, height = h }),
        };
    }
}
