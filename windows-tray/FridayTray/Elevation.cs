using System.ComponentModel;
using System.Diagnostics;
using System.Security.Principal;
using System.Text;
using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// "Start FRIDAY with administrator rights": a scheduled task that runs the
/// tray at logon at the account's highest level (see <see cref="ElevatedStart"/>).
/// Making or removing it needs administrator rights, so the tray runs itself
/// again with `--apply-elevated on|off` through one Windows prompt; running the
/// task at every sign-in never prompts.
/// </summary>
internal static class Elevation
{
    public static bool IsElevated()
    {
        using var id = WindowsIdentity.GetCurrent();
        return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);
    }

    public static bool Enabled() => Schtasks("/Query", "/TN", ElevatedStart.TaskName).Code == 0;

    /// <summary>
    /// Turns it on or off through one UAC prompt. False when the person
    /// declined the prompt or Windows refused; the settings are left as they were.
    /// </summary>
    public static bool Apply(Install install, bool on)
    {
        using var me = WindowsIdentity.GetCurrent();
        try
        {
            using var p = Process.Start(new ProcessStartInfo(install.TrayExe)
            {
                UseShellExecute = true,
                Verb = "runas",
                WindowStyle = ProcessWindowStyle.Hidden,
                Arguments = $"--apply-elevated {(on ? "on" : "off")} \"{me.Name}\"",
            });
            if (p is null || !p.WaitForExit(60_000))
                return false;
            return p.ExitCode == 0 && Enabled() == on;
        }
        catch (Win32Exception)
        {
            // 1223: the prompt was declined.
            return false;
        }
    }

    /// <summary>The elevated half: writes or deletes the task for <paramref name="user"/>. Exit code 0 on success.</summary>
    public static int ApplyElevated(Install install, bool on, string user)
    {
        if (!on)
        {
            var del = Schtasks("/Delete", "/TN", ElevatedStart.TaskName, "/F");
            return del.Code == 0 || !Enabled() ? 0 : 1;
        }
        var xml = Path.Combine(Path.GetTempPath(), $"friday-task-{Environment.ProcessId}.xml");
        try
        {
            File.WriteAllText(xml, ElevatedStart.TaskXml(user, install.TrayExe), Encoding.Unicode);
            return Schtasks("/Create", "/TN", ElevatedStart.TaskName, "/XML", xml, "/F").Code == 0 ? 0 : 1;
        }
        finally
        {
            try { File.Delete(xml); } catch (IOException) { }
        }
    }

    /// <summary>
    /// Starts FRIDAY again in a moment, from a short-lived shell so the running
    /// tray can exit first: through its task when administrator start-up is on,
    /// else the program itself.
    /// </summary>
    public static void RestartSoon(Install install)
    {
        var start = Enabled()
            ? $"schtasks /Run /TN {ElevatedStart.TaskName}"
            : $"start \"\" \"{install.TrayExe}\"";
        Process.Start(new ProcessStartInfo("cmd.exe", $"/c timeout /t 2 /nobreak >nul & {start}")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
        });
    }

    private static (int Code, string Output) Schtasks(params string[] args)
    {
        var info = new ProcessStartInfo("schtasks.exe")
        {
            CreateNoWindow = true,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        foreach (var a in args)
            info.ArgumentList.Add(a);
        using var p = Process.Start(info)!;
        var output = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
        p.WaitForExit();
        return (p.ExitCode, output);
    }
}
