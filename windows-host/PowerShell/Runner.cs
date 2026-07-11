using System.Diagnostics;
using System.Text;

namespace WindowsHost.PowerShell;

/// <summary>
/// Runs PowerShell for composite/privileged operations the planner cannot
/// express as a single window intent. No permission logic — the caller already
/// passed the upstream gate.
/// </summary>
public static class Runner
{
    public static string Invoke(string script)
    {
        var psi = new ProcessStartInfo("pwsh", "-NoProfile -NonInteractive -Command " + Quote(script))
        {
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
        };
        using var p = Process.Start(psi)
            ?? throw new System.ComponentModel.Win32Exception(2, "pwsh not found");
        var output = p.StandardOutput.ReadToEnd();
        var error = p.StandardError.ReadToEnd();
        p.WaitForExit();
        return string.IsNullOrEmpty(error) ? output : $"ERROR: {error}";
    }

    private static string Quote(string s) => "\"" + s.Replace("\"", "\\\"") + "\"";
}
