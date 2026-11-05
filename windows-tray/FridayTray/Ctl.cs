using System.Diagnostics;
using System.Text;

namespace FridayTray;

/// <summary>Runs friday-ctl and collects everything it prints.</summary>
internal static class Ctl
{
    public static async Task<(bool ok, string output)> RunAsync(Install install, IEnumerable<string> args, TimeSpan timeout)
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
            return (false, $"cannot reach daemon: could not start friday-ctl ({e.Message})");
        }
        var stdout = p.StandardOutput.ReadToEndAsync();
        var stderr = p.StandardError.ReadToEndAsync();
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            await p.WaitForExitAsync(cts.Token);
        }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            return (false, (await stdout) + (await stderr) + $"\ntimed out after {(int)timeout.TotalSeconds}s waiting for friday-ctl");
        }
        return (p.ExitCode == 0, (await stdout) + (await stderr));
    }
}
