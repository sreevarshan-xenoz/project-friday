using System.Diagnostics;
using System.Text;

namespace FridayTray;

internal readonly record struct CtlResult(bool Ok, string Output, int ExitCode)
{
    public void Deconstruct(out bool ok, out string output)
    {
        ok = Ok;
        output = Output;
    }

    public void Deconstruct(out bool ok, out string output, out int exitCode)
    {
        ok = Ok;
        output = Output;
        exitCode = ExitCode;
    }
}

/// <summary>Runs friday-ctl and collects everything it prints.</summary>
internal static class Ctl
{
    /// <param name="onLine">Called with each line friday-ctl prints, as it prints it.</param>
    public static async Task<CtlResult> RunAsync(Install install, IEnumerable<string> args, TimeSpan timeout,
        Action<string>? onLine = null)
    {
        var psi = new ProcessStartInfo(install.Ctl)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (var a in args)
            psi.ArgumentList.Add(a);
        foreach (var (k, v) in install.ChildEnvironment())
            psi.Environment[k] = v;

        using var p = new Process { StartInfo = psi };
        try
        {
            p.Start();
        }
        catch (Exception e)
        {
            return new CtlResult(false, $"cannot reach daemon: could not start friday-ctl ({e.Message})", -1);
        }
        var stdout = ReadLinesAsync(p.StandardOutput, onLine);
        var stderr = p.StandardError.ReadToEndAsync();
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            await p.WaitForExitAsync(cts.Token);
        }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            return new CtlResult(false, (await stdout) + (await stderr) + $"\ntimed out after {(int)timeout.TotalSeconds}s waiting for friday-ctl", -1);
        }
        return new CtlResult(p.ExitCode == 0, (await stdout) + (await stderr), p.ExitCode);
    }

    private static async Task<string> ReadLinesAsync(StreamReader reader, Action<string>? onLine)
    {
        var all = new StringBuilder();
        while (await reader.ReadLineAsync() is { } line)
        {
            all.Append(line).Append('\n');
            onLine?.Invoke(line);
        }
        return all.ToString();
    }
}
