using FridayTray;
using FridayTray.Core;

/// <summary>
/// friday-tray.exe: FRIDAY on Windows for the signed-in user.
///   (no arguments)  the tray app; one per user session
///   --apply-elevated on|off <DOMAIN\user>
///                   (run through one UAC prompt) create or remove the
///                   scheduled task that starts FRIDAY as administrator
///   --screenshots <folder>
///                   open each window with sample content, save a picture of
///                   it in the folder, exit (CI looks at the Windows side this way)
///   --smoke         headless check for CI and installers: start the daemon and
///                   host, wait until state.json reports "running" and
///                   friday-ctl reaches the daemon, stop them, exit 0 (or 1);
///                   the steps go to %LOCALAPPDATA%\friday\logs\smoke.log
/// </summary>
internal static class Program
{
    public const string OpenEventName = @"Local\FRIDAY.Tray.Open";

    [STAThread]
    private static int Main(string[] args)
    {
        var install = new Install(Paths.ForCurrentUser());
        var shots = Array.IndexOf(args, "--screenshots");
        if (shots >= 0 && args.Length > shots + 1)
            return Screenshots.Run(install, args[shots + 1]);
        if (args.Contains("--smoke"))
            return Smoke(install).GetAwaiter().GetResult();
        // Settings' "Install in <browser>", without the window: for checking
        // it on a test machine. Prints what it did.
        var ext = Array.IndexOf(args, "--install-extension");
        if (ext >= 0 && args.Length > ext + 1)
        {
            var b = BrowserLink.All.FirstOrDefault(x => x.Name.Contains(args[ext + 1], StringComparison.OrdinalIgnoreCase));
            if (b is null || b.Firefox)
            {
                Console.Error.WriteLine("usage: friday-tray --install-extension <Chrome|Edge|Brave|Vivaldi>");
                return 2;
            }
            using var cts = new CancellationTokenSource(TimeSpan.FromMinutes(2));
            Selection.TrySetText(BrowserSetup.ExtensionDir(install));
            // UI Automation from a blocked STA thread can hang: run it on the pool.
            var said = Task.Run(() => ExtensionInstaller.InstallAsync(b, install, cts.Token)).GetAwaiter().GetResult();
            Console.WriteLine(said);
            return said.Contains("is on in", StringComparison.Ordinal) ? 0 : 1;
        }
        // The elevated half of "Start with administrator rights": run with the
        // person's one Windows prompt, do the one thing, exit.
        var apply = Array.IndexOf(args, "--apply-elevated");
        if (apply >= 0 && args.Length > apply + 2)
            return Elevation.ApplyElevated(install, args[apply + 1] == "on", args[apply + 2]);

        using var single = new Mutex(true, @"Local\FRIDAY.Tray", out var first);
        if (!first)
        {
            // FRIDAY is already running: ask it to open its window (the Start menu entry).
            // This copy was just started by the person, so it may bring a window
            // forward; let the running one do that, or its window opens behind.
            FridayTray.Native.AllowSetForegroundWindow(FridayTray.Native.ASFW_ANY);
            try { System.Threading.EventWaitHandle.OpenExisting(OpenEventName).Set(); }
            catch (Exception e) when (e is System.Threading.WaitHandleCannotBeOpenedException or UnauthorizedAccessException) { }
            return 0;
        }
        ApplicationConfiguration.Initialize();
        FridayTray.Fluent.FluentApp.Init();
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
        Application.Run(new TrayContext(install));
        return 0;
    }

    private static async Task<int> Smoke(Install install)
    {
        // A GUI-subsystem exe has no console, so the result also goes to a file.
        Directory.CreateDirectory(install.Paths.LogDir);
        var logFile = Path.Combine(install.Paths.LogDir, "smoke.log");
        File.WriteAllText(logFile, "");
        void Say(string s)
        {
            Console.WriteLine($"[smoke] {s}");
            File.AppendAllText(logFile, $"[smoke] {s}{Environment.NewLine}");
        }
        foreach (var exe in new[] { install.Daemon, install.Host, install.Ctl })
        {
            if (!File.Exists(exe)) { Say($"missing {exe}"); return 1; }
        }
        // A state.json left by an earlier daemon says "ready" with that daemon's
        // token: waiting on it would pass before this daemon is up.
        File.Delete(install.Paths.StateFile);
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
