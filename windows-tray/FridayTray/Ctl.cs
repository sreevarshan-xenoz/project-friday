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
    /// <param name="cancel">Stops friday-ctl (a newer question replaced this one).</param>
    public static async Task<CtlResult> RunAsync(Install install, IEnumerable<string> args, TimeSpan timeout,
        Action<string>? onLine = null, CancellationToken cancel = default)
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
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancel);
        cts.CancelAfter(timeout);
        try
        {
            await p.WaitForExitAsync(cts.Token);
        }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            if (cancel.IsCancellationRequested)
                return new CtlResult(false, "cancelled: a newer question replaced this one", -2);
            return new CtlResult(false, (await stdout) + (await stderr) + $"\ntimed out after {(int)timeout.TotalSeconds}s waiting for friday-ctl", -1);
        }
        return new CtlResult(p.ExitCode == 0, (await stdout) + (await stderr), p.ExitCode);
    }

    /// <summary>Reads <paramref name="text"/> aloud (a new one replaces the one being read). Doesn't wait.</summary>
    public static void Say(Install install, string text)
    {
        var psi = new ProcessStartInfo(install.Ctl)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            StandardInputEncoding = new UTF8Encoding(false),
        };
        psi.ArgumentList.Add("say");
        psi.ArgumentList.Add("-");
        foreach (var (k, v) in install.ChildEnvironment())
            psi.Environment[k] = v;
        try
        {
            var p = Process.Start(psi);
            p?.StandardInput.Write(text);
            p?.StandardInput.Close();
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException)
        {
            // No voice is not worth interrupting an answer for.
        }
    }

    /// <summary>
    /// Reads <paramref name="text"/> aloud and waits: 0 when it was said to
    /// the end, other when it was stopped or couldn't be said.
    /// </summary>
    public static async Task<int> SayAsync(Install install, string text)
    {
        var psi = new ProcessStartInfo(install.Ctl)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            StandardInputEncoding = new UTF8Encoding(false),
        };
        psi.ArgumentList.Add("say");
        psi.ArgumentList.Add("-");
        foreach (var (k, v) in install.ChildEnvironment())
            psi.Environment[k] = v;
        try
        {
            using var p = Process.Start(psi);
            if (p is null)
                return -1;
            await p.StandardInput.WriteAsync(text);
            p.StandardInput.Close();
            await p.WaitForExitAsync();
            return p.ExitCode;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or IOException)
        {
            return -1;
        }
    }

    /// <summary>Stops whatever FRIDAY is reading aloud.</summary>
    public static void StopSaying(Install install)
    {
        var psi = new ProcessStartInfo(install.Ctl, "say --stop") { UseShellExecute = false, CreateNoWindow = true };
        foreach (var (k, v) in install.ChildEnvironment())
            psi.Environment[k] = v;
        try { Process.Start(psi); }
        catch (System.ComponentModel.Win32Exception) { }
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
