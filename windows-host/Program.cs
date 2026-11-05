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
        // The focused window's bounds, so a screen ask can capture just it.
        if (args.Length > 0 && args[0] == "--active-window")
            return OperatingSystem.IsWindows() ? ActiveWindow.Run() : 2;

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
            using var pipe = new NamedPipeServerStream(
                pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
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

    private static void ReadExact(Stream stream, byte[] buffer, int count)
    {
        var read = 0;
        while (read < count)
        {
            var n = stream.Read(buffer, read, count - read);
            if (n == 0)
                throw new EndOfStreamException("pipe closed mid-frame");
            read += n;
        }
    }
}
