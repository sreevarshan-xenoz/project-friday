using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace WindowsHost;

/// <summary>
/// `friday-windows-host --screenshot [x y width height]`: write a PNG of the
/// whole virtual desktop (every monitor), or of the given region in desktop
/// coordinates, to stdout, and exit. This is FRIDAY's screen capture on
/// Windows: the daemon's context collector runs it the way it runs `grim` on
/// Linux, so "ask about the screen" works on both. It only runs when asked,
/// and the daemon only asks after the user's per-use consent.
/// </summary>
[SupportedOSPlatform("windows")]
public static class Screenshot
{
    private const int SM_XVIRTUALSCREEN = 76, SM_YVIRTUALSCREEN = 77, SM_CXVIRTUALSCREEN = 78, SM_CYVIRTUALSCREEN = 79;
    // Per-monitor-v2 DPI awareness, so scaled displays are captured whole and
    // at full resolution instead of in virtualized (scaled-down) coordinates.
    private static readonly IntPtr DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = new(-4);

    [DllImport("user32.dll")]
    private static extern bool SetProcessDpiAwarenessContext(IntPtr value);

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    /// <summary>Parse the arguments after `--screenshot`: nothing (whole desktop) or `x y width height`.</summary>
    public static Rectangle? ParseRegion(string[] args)
    {
        if (args.Length == 0)
            return null;
        if (args.Length == 4 && args.All(a => int.TryParse(a, out _)))
        {
            var v = args.Select(int.Parse).ToArray();
            if (v[2] > 0 && v[3] > 0)
                return new Rectangle(v[0], v[1], v[2], v[3]);
        }
        throw new ArgumentException("usage: --screenshot [x y width height] (width and height > 0)");
    }

    public static int Run(string[] args)
    {
        Rectangle? region;
        try
        {
            region = ParseRegion(args);
        }
        catch (ArgumentException e)
        {
            Console.Error.WriteLine(e.Message);
            return 2;
        }

        try
        {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        catch (EntryPointNotFoundException)
        {
            // Older Windows: capture in whatever scaling the system applies.
        }

        var bounds = region ?? new Rectangle(
            GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        if (bounds.Width <= 0 || bounds.Height <= 0)
        {
            Console.Error.WriteLine("no screen to capture (no interactive desktop?)");
            return 1;
        }

        try
        {
            using var bitmap = new Bitmap(bounds.Width, bounds.Height, PixelFormat.Format32bppArgb);
            using (var g = Graphics.FromImage(bitmap))
                g.CopyFromScreen(bounds.Left, bounds.Top, 0, 0, bounds.Size, CopyPixelOperation.SourceCopy);
            using var png = new MemoryStream();
            bitmap.Save(png, ImageFormat.Png);
            using var stdout = Console.OpenStandardOutput();
            png.WriteTo(stdout);
            stdout.Flush();
            return 0;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or ExternalException or ArgumentException)
        {
            Console.Error.WriteLine($"screen capture failed: {e.Message}");
            return 1;
        }
    }
}
