using FridayTray.Core;
using Microsoft.Win32;

namespace FridayTray;

/// <summary>
/// The installed layout: every FRIDAY program sits next to friday-tray.exe
/// (the MSI installs them together), and the environment they share.
/// </summary>
internal sealed class Install
{
    public Install(Paths paths)
    {
        Paths = paths;
        Dir = AppContext.BaseDirectory;
    }

    public Paths Paths { get; }
    public string Dir { get; }
    public string Daemon => Path.Combine(Dir, "friday-daemon.exe");
    public string Host => Path.Combine(Dir, "friday-windows-host.exe");
    public string Ctl => Path.Combine(Dir, "friday-ctl.exe");
    public string TrayExe => Environment.ProcessPath ?? Path.Combine(Dir, "friday-tray.exe");

    /// <summary>
    /// What the daemon and friday-ctl run with: where state and memory live,
    /// plus the user's AI provider settings from friday.env.
    /// </summary>
    public IDictionary<string, string> ChildEnvironment()
    {
        var env = new Dictionary<string, string>(EnvFile.Load(Paths.EnvFile))
        {
            ["FRIDAY_STATE_FILE"] = Paths.StateFile,
            ["FRIDAY_MEMORY_DIR"] = Paths.MemoryDir,
        };
        return env;
    }

    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string RunValue = "FRIDAY";

    /// <summary>Start at login: the HKCU Run entry the installer also writes.</summary>
    public bool StartsAtLogin
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(RunValue) is string;
        }
        set
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (value)
                key.SetValue(RunValue, $"\"{TrayExe}\"");
            else
                key.DeleteValue(RunValue, throwOnMissingValue: false);
        }
    }
}
