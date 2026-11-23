using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Runtime.Versioning;
using Windows.Graphics.Imaging;
using Windows.Media.Ocr;
using Windows.Storage.Streams;

namespace WindowsHost;

/// <summary>
/// `friday-windows-host --ocr`: read a PNG from stdin and print its words,
/// one line of text per line, with Windows' own OCR (the languages in the
/// user's profile). This is the Windows counterpart of `tesseract stdin
/// stdout`: a window whose app gives no text (Electron, a canvas, a remote
/// desktop) is asked about with its words instead of its picture. Nothing
/// leaves this computer here. Exit 1 when there's no OCR language or the
/// picture can't be read.
/// </summary>
[SupportedOSPlatform("windows10.0.19041.0")]
public static class Ocr
{
    public static int Run()
    {
        using var input = Console.OpenStandardInput();
        using var buffer = new MemoryStream();
        input.CopyTo(buffer);
        try
        {
            var text = ReadAsync(buffer.ToArray()).GetAwaiter().GetResult();
            if (text is null)
                return 1;
            Console.OutputEncoding = System.Text.Encoding.UTF8;
            Console.Out.Write(text);
            return 0;
        }
        catch (Exception e) when (e is COMException or ArgumentException or InvalidOperationException)
        {
            return 1;
        }
    }

    private static async Task<string?> ReadAsync(byte[] png)
    {
        var engine = OcrEngine.TryCreateFromUserProfileLanguages();
        if (engine is null || png.Length == 0)
            return null;
        using var stream = new InMemoryRandomAccessStream();
        await stream.WriteAsync(png.AsBuffer());
        stream.Seek(0);
        var decoder = await BitmapDecoder.CreateAsync(stream);
        // The engine refuses pictures past its limit: scale a big screen down.
        var max = OcrEngine.MaxImageDimension;
        var scale = Math.Min(1.0, (double)max / Math.Max(decoder.PixelWidth, decoder.PixelHeight));
        var transform = new BitmapTransform
        {
            ScaledWidth = (uint)Math.Max(1, decoder.PixelWidth * scale),
            ScaledHeight = (uint)Math.Max(1, decoder.PixelHeight * scale),
            InterpolationMode = BitmapInterpolationMode.Fant,
        };
        using var bitmap = await decoder.GetSoftwareBitmapAsync(
            BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, transform,
            ExifOrientationMode.IgnoreExifOrientation, ColorManagementMode.DoNotColorManage);
        var result = await engine.RecognizeAsync(bitmap);
        return string.Join("\n", result.Lines.Select(l => l.Text));
    }
}
