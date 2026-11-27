using System.Security;

namespace FridayTray.Core;

/// <summary>
/// Starting FRIDAY with administrator rights without a Windows prompt at every
/// sign-in: a scheduled task that runs the tray at logon at the highest
/// privilege the account has. Creating it needs administrator rights once (the
/// one prompt, when the person turns this on); running it never prompts.
/// The point is that an app running as administrator can only be controlled
/// (typed into, closed, focused) by a program running as administrator too.
/// </summary>
public static class ElevatedStart
{
    public const string TaskName = "FRIDAY";

    /// <summary>
    /// Where the task's copy of FRIDAY lives: under Program Files, which only
    /// administrators can change. The per-user install is writable by any
    /// program the person runs, so a task starting it with administrator
    /// rights would hand those rights to whatever replaced the exe there.
    /// </summary>
    public static string MachineDir(string programFiles) => Path.Combine(programFiles, "FRIDAY", "Administrator start");

    /// <summary>The file in the copy naming the install it was made from.</summary>
    public const string SourceFile = "copied-from.txt";

    /// <summary>Replaces <paramref name="to"/> with a copy of every file under <paramref name="from"/>.</summary>
    public static void CopyInstall(string from, string to)
    {
        if (Directory.Exists(to))
            Directory.Delete(to, recursive: true);
        foreach (var dir in Directory.EnumerateDirectories(from, "*", SearchOption.AllDirectories))
            Directory.CreateDirectory(Path.Combine(to, Path.GetRelativePath(from, dir)));
        Directory.CreateDirectory(to);
        foreach (var file in Directory.EnumerateFiles(from, "*", SearchOption.AllDirectories))
            File.Copy(file, Path.Combine(to, Path.GetRelativePath(from, file)), overwrite: true);
        File.WriteAllText(Path.Combine(to, SourceFile), from);
    }

    /// <summary>
    /// Whether the copy in <paramref name="copyDir"/> is older than the install
    /// it came from (FRIDAY was updated since): compared by the tray exe's
    /// file version. False when there is no copy or no source to compare.
    /// </summary>
    public static bool CopyIsOld(string copyDir, Func<string, string?> fileVersion)
    {
        var source = Path.Combine(copyDir, SourceFile);
        if (!File.Exists(source))
            return false;
        var from = File.ReadAllText(source).Trim();
        var theirs = fileVersion(Path.Combine(from, "friday-tray.exe"));
        var ours = fileVersion(Path.Combine(copyDir, "friday-tray.exe"));
        return theirs is not null && ours is not null && theirs != ours;
    }

    /// <summary>The task definition for <paramref name="user"/> (DOMAIN\name) starting <paramref name="exe"/>.</summary>
    public static string TaskXml(string user, string exe)
    {
        var u = SecurityElement.Escape(user);
        var command = SecurityElement.Escape(exe);
        var dir = SecurityElement.Escape(Path.GetDirectoryName(exe) ?? "");
        return $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <RegistrationInfo>
                <Description>Starts FRIDAY with administrator rights when you sign in, so it can control apps running as administrator.</Description>
              </RegistrationInfo>
              <Triggers>
                <LogonTrigger>
                  <Enabled>true</Enabled>
                  <UserId>{u}</UserId>
                </LogonTrigger>
              </Triggers>
              <Principals>
                <Principal id="Author">
                  <UserId>{u}</UserId>
                  <LogonType>InteractiveToken</LogonType>
                  <RunLevel>HighestAvailable</RunLevel>
                </Principal>
              </Principals>
              <Settings>
                <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
                <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
                <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
                <AllowHardTerminate>true</AllowHardTerminate>
                <StartWhenAvailable>true</StartWhenAvailable>
                <AllowStartOnDemand>true</AllowStartOnDemand>
                <Enabled>true</Enabled>
                <Hidden>false</Hidden>
                <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
                <Priority>7</Priority>
              </Settings>
              <Actions Context="Author">
                <Exec>
                  <Command>{command}</Command>
                  <WorkingDirectory>{dir}</WorkingDirectory>
                </Exec>
              </Actions>
            </Task>
            """;
    }
}
