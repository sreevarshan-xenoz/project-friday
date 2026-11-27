using System;
using System.Buffers;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using WindowsHost;
using WindowsHost.Actions;
using WindowsHost.IPC;
using WindowsHost.Security;
using WindowsHost.Win32;

/// <summary>
/// FRIDAY Windows Host Service.
///
/// A standalone microservice. It listens on a named pipe, decodes versioned
/// BridgeFrames, validates them, dispatches each intent to a handler that calls
/// Win32 / UI Automation / COM / PowerShell, and returns a normalized
/// BridgeResponse. It contains no planner, memory, AI, or permission logic.
/// </summary>
public static class Program
{
    public static int Main(string[] args)
    {
        // One-shot screen capture for the daemon's context collector (the
        // Windows counterpart of `grim -`); not the pipe service.
        if (args.Length > 0 && args[0] == "--screenshot")
            return OperatingSystem.IsWindows() ? Screenshot.Run(args[1..]) : 2;
        // Words read off a PNG on stdin (Windows OCR), for a window whose
        // app gives no text; the counterpart of `tesseract stdin stdout`.
        if (args.Length > 0 && args[0] == "--ocr")
            return OperatingSystem.IsWindowsVersionAtLeast(10, 0, 19041) ? Ocr.Run() : 2;
        // The focused window's bounds, so a screen ask can capture just it.
        if (args.Length > 0 && args[0] == "--active-window")
            return OperatingSystem.IsWindows() ? ActiveWindow.Run() : 2;

        // What "open <name>" would start, without starting it (used by CI and
        // for "why can't FRIDAY open X").
        if (args.Length > 1 && args[0] == "--resolve")
        {
            var found = OperatingSystem.IsWindows() ? AppResolver.Resolve(args[1]) : null;
            Console.WriteLine(found is null ? "not found" : $"{found.Via}: {found.FileName}");
            return found is null ? 1 : 0;
        }

        var session = Environment.GetEnvironmentVariable("FRIDAY_SESSION") ?? "default";
        var pipeName = $"friday-windows-{session}";
        var dispatcher = new Dispatcher();
        var manifest = Manifest.Load();
        // One registry for the host's lifetime: stable window ids (returned to
        // Rust) must resolve across every pipe connection, not per-request.
        var registry = new WindowRegistry();

        Console.WriteLine($"[windows-host] listening on \\\\.\\pipe\\{pipeName}");
        while (true)
        {
            // CurrentUserOnly: only this person's processes can connect (the
            // pipe's ACL), and a pipe of that name another user made is refused.
            using var pipe = new NamedPipeServerStream(
                pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            try
            {
                pipe.WaitForConnection();
                HandleClient(pipe, dispatcher, manifest, registry);
            }
            catch (Exception ex)
            {
                // A bad client must never take down the host.
                Console.Error.WriteLine($"[windows-host] client error: {ex}");
            }
            finally
            {
                if (pipe.IsConnected)
                    pipe.Disconnect();
            }
        }
    }

    private static void HandleClient(NamedPipeServerStream pipe, Dispatcher dispatcher, JsonElement manifest, WindowRegistry registry)
    {
        // 4-byte little-endian length prefix + UTF-8 JSON (matches Rust wire format).
        var lenBytes = ArrayPool<byte>.Shared.Rent(4);
        try
        {
            ReadExact(pipe, lenBytes, 4);
            var len = BitConverter.ToInt32(lenBytes, 0);
            if (len <= 0 || len > MaxFrame)
                throw new InvalidDataException($"frame length {len} out of range");
            var json = ArrayPool<byte>.Shared.Rent(len);
            try
            {
                ReadExact(pipe, json, len);
                var frame = FrameCodec.Decode(json.AsSpan(0, len).ToArray());

                var validation = Validator.Validate(frame);
                BridgeResponse response = validation != null
                    ? new BridgeResponse { Ok = false, Error = validation }
                    : frame.Message.Action == "windows.manifest"
                        ? new BridgeResponse { Ok = true, Output = manifest }
                        : dispatcher.Dispatch(frame.Message.Action, frame.Message.Payload, registry);

                var respFrame = FrameCodec.Respond(frame, response);
                var outBytes = FrameCodec.Encode(respFrame);
                pipe.Write(BitConverter.GetBytes(outBytes.Length), 0, 4);
                pipe.Write(outBytes, 0, outBytes.Length);
                pipe.Flush();
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(json);
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(lenBytes);
        }
    }

    /// <summary>The largest request frame (a manifest or a screenshot answer is far smaller).</summary>
    private const int MaxFrame = 16 * 1024 * 1024;

    /// <summary>
    /// Reads exactly <paramref name="count"/> bytes, giving up after 5 s: a
    /// client that connects and sends nothing must not hold the host (it
    /// serves one client at a time).
    /// </summary>
    private static void ReadExact(Stream stream, byte[] buffer, int count)
    {
        var read = 0;
        using var timeout = new System.Threading.CancellationTokenSource(TimeSpan.FromSeconds(5));
        while (read < count)
        {
            var n = stream.ReadAsync(buffer.AsMemory(read, count - read), timeout.Token).AsTask().GetAwaiter().GetResult();
            if (n == 0)
                throw new EndOfStreamException("pipe closed mid-frame");
            read += n;
        }
    }
}
