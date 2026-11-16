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
