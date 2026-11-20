using System.Diagnostics;
using System.Text;

namespace FridayTray;

internal static class ProcessRunner
{
    /// <summary>Checks whether `ollama` is installed on PATH or in standard per-user app data.</summary>
    public static bool IsOllamaInstalled(out string exePath)
    {
        // Check standard Windows per-user location first in case PATH in the running process is stale
        var localApp = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (!string.IsNullOrEmpty(localApp))
        {
            var userOllama = Path.Combine(localApp, "Programs", "Ollama", "ollama.exe");
            if (File.Exists(userOllama))
            {
                exePath = userOllama;
                return true;
            }
        }

        // Check PATH
        var pathEnv = Environment.GetEnvironmentVariable("PATH") ?? "";
        foreach (var dir in pathEnv.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            var trimmed = dir.Trim();
            var candidate = Path.Combine(trimmed, "ollama.exe");
            if (File.Exists(candidate))
            {
                exePath = candidate;
                return true;
            }
            var noExt = Path.Combine(trimmed, "ollama");
            if (File.Exists(noExt))
            {
                exePath = noExt;
                return true;
            }
        }

        exePath = "ollama";
        return false;
    }

    /// <summary>
    /// Makes sure Ollama's server answers on 127.0.0.1:11434 (a fresh winget
    /// install doesn't always start it): starts `ollama serve` hidden and
    /// waits up to 20 seconds.
    /// </summary>
    public static async Task<bool> EnsureOllamaRunningAsync(string exe)
    {
        using var http = new System.Net.Http.HttpClient { Timeout = TimeSpan.FromSeconds(2) };
        async Task<bool> Up()
        {
            try { return (await http.GetAsync("http://127.0.0.1:11434/")).IsSuccessStatusCode; }
            catch (Exception) { return false; }
        }
        if (await Up())
            return true;
        try
        {
            Process.Start(new ProcessStartInfo(exe, "serve") { UseShellExecute = false, CreateNoWindow = true });
        }
        catch (Exception)
        {
            return false;
        }
        for (var i = 0; i < 20; i++)
        {
            await Task.Delay(1000);
            if (await Up())
                return true;
        }
        return false;
    }

    /// <summary>Runs an executable redirecting output and delivering each line or progress update to <paramref name="onOutput"/>.</summary>
    public static async Task<(bool Ok, string Output)> RunAsync(
        string exe,
        IEnumerable<string> args,
        TimeSpan timeout,
        Action<string>? onOutput = null)
    {
        var psi = new ProcessStartInfo(exe)
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

        using var p = new Process { StartInfo = psi };
        try
        {
            p.Start();
        }
        catch (Exception e)
        {
            return (false, e.Message);
        }

        var allOutput = new StringBuilder();
        var stdoutTask = ReadStreamProgressAsync(p.StandardOutput, allOutput, onOutput);
        var stderrTask = ReadStreamProgressAsync(p.StandardError, allOutput, onOutput);

        using var cts = new CancellationTokenSource(timeout);
        try
        {
            await p.WaitForExitAsync(cts.Token);
            await Task.WhenAll(stdoutTask, stderrTask);
        }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            return (false, "Operation timed out.");
        }

        return (p.ExitCode == 0, allOutput.ToString().Trim());
    }

    private static async Task ReadStreamProgressAsync(
        StreamReader reader,
        StringBuilder allOutput,
        Action<string>? onOutput)
    {
        var buffer = new char[512];
        var segment = new StringBuilder();
        int read;
        while ((read = await reader.ReadAsync(buffer, 0, buffer.Length)) > 0)
        {
            for (int i = 0; i < read; i++)
            {
                char c = buffer[i];
                if (c is '\r' or '\n')
                {
                    if (segment.Length > 0)
                    {
                        var line = segment.ToString().Trim();
                        lock (allOutput)
                        {
                            allOutput.AppendLine(line);
                        }
                        if (line.Length > 0)
                            onOutput?.Invoke(line);
                        segment.Clear();
                    }
                }
                else
                {
                    segment.Append(c);
                }
            }
        }
        if (segment.Length > 0)
        {
            var line = segment.ToString().Trim();
            lock (allOutput)
            {
                allOutput.AppendLine(line);
            }
            if (line.Length > 0)
                onOutput?.Invoke(line);
        }
    }
}
