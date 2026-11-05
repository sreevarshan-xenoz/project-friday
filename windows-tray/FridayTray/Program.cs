using FridayTray;
using FridayTray.Core;

/// <summary>
/// friday-tray.exe: FRIDAY on Windows for the signed-in user.
///   (no arguments)  the tray app; one per user session
///   --smoke         headless check for CI and installers: start the daemon and
///                   host, wait until state.json reports "running" and
///                   friday-ctl reaches the daemon, stop them, exit 0 (or 1)
/// </summary>
internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        var install = new Install(Paths.ForCurrentUser());
        if (args.Contains("--smoke"))
            return Smoke(install).GetAwaiter().GetResult();

        using var single = new Mutex(true, @"Local\FRIDAY.Tray", out var first);
        if (!first)
            return 0;
        ApplicationConfiguration.Initialize();
        Application.Run(new TrayContext(install));
        return 0;
    }

    private static async Task<int> Smoke(Install install)
    {
        void Say(string s) => Console.WriteLine($"[smoke] {s}");
        foreach (var exe in new[] { install.Daemon, install.Host, install.Ctl })
        {
            if (!File.Exists(exe)) { Say($"missing {exe}"); return 1; }
        }
        using var daemon = new Supervised("friday-daemon", install.Daemon, install.ChildEnvironment, install.Paths.LogDir);
        using var host = new Supervised("friday-windows-host", install.Host, install.ChildEnvironment, install.Paths.LogDir);
        host.Start();
        daemon.Start();
        var ok = false;
        for (var i = 0; i < 90 && !ok; i++)
        {
            await Task.Delay(1000);
            ok = DaemonState.Read(install.Paths.StateFile).Mode is "ready" or "working";
        }
        if (!ok) { Say($"daemon never reported running in {install.Paths.StateFile}"); return 1; }
        Say($"state.json says running: {install.Paths.StateFile}");
        var (ctlOk, output) = await Ctl.RunAsync(install, ["system-info"], TimeSpan.FromSeconds(30));
        if (!ctlOk) { Say($"friday-ctl system-info failed: {output}"); return 1; }
        Say("friday-ctl reached the daemon");
        if (!host.Running) { Say("friday-windows-host is not running"); return 1; }
        Say("friday-windows-host is running");
        Say("ok");
        return 0;
    }
}
