using System.Text.Json;

namespace FridayTray.Core;

/// <summary>An update in flight: the version it left, the one it installs, and both MSIs.</summary>
public sealed record RollbackPlan(string From, string To, string OldMsi, string NewMsi, DateTime At, bool Reverted = false);

public enum RollbackStep
{
    /// <summary>Nothing to do.</summary>
    None,
    /// <summary>The update worked (or never ran): forget the plan.</summary>
    Clear,
    /// <summary>The new version is running; its health isn't known yet.</summary>
    Wait,
    /// <summary>The new version didn't start properly: reinstall the old one.</summary>
    Revert,
    /// <summary>Back on the old version after a revert: say so, then forget.</summary>
    TellReverted,
}

/// <summary>
/// A bad update reverts itself. Before the new MSI runs, the MSI of the version
/// installed now is downloaded (checked like any update) and kept; the new
/// version's first start then has two minutes for its daemon to say it is
/// running, or the old version goes back on.
/// </summary>
public static class Rollback
{
    /// <summary>How long a new version's daemon has to come up.</summary>
    public static readonly TimeSpan Grace = TimeSpan.FromMinutes(2);

    public static RollbackPlan? Read(string file)
    {
        try
        {
            return File.Exists(file) ? JsonSerializer.Deserialize<RollbackPlan>(File.ReadAllText(file)) : null;
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    public static void Write(string file, RollbackPlan plan)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        File.WriteAllText(file, JsonSerializer.Serialize(plan));
    }

    /// <summary>x.y.z without a pre-release part: what the running tray calls its version.</summary>
    public static string Plain(string version) => version.Split('-')[0];

    /// <summary>What to do now, running <paramref name="current"/>; <paramref name="healthy"/>
    /// is null until the daemon has come up or the grace time ran out.</summary>
    public static RollbackStep Decide(RollbackPlan? plan, string current, bool? healthy, DateTime now)
    {
        if (plan is null)
            return RollbackStep.None;
        current = Plain(current);
        if (current == Plain(plan.From))
            // Reverted: say so. Otherwise the update hasn't run (yet): a day
            // later it never will (the installer was cancelled).
            return plan.Reverted ? RollbackStep.TellReverted
                : now - plan.At > TimeSpan.FromDays(1) ? RollbackStep.Clear : RollbackStep.None;
        if (current != Plain(plan.To) || plan.Reverted)
            return RollbackStep.Clear;
        return healthy switch
        {
            true => RollbackStep.Clear,
            false => RollbackStep.Revert,
            null => RollbackStep.Wait,
        };
    }

    /// <summary>Whether <paramref name="msi"/> is a FRIDAY MSI kept in <paramref name="dir"/>
    /// (its old or new folder) that the signed SHA256SUMS beside it still vouches for: the
    /// plan is a file anyone running as the person could edit, so it is checked again
    /// right before a revert runs it.</summary>
    public static bool Verified(string dir, string msi, string publicKey = Updates.UpdateKey)
    {
        try
        {
            var full = Path.GetFullPath(msi);
            var folder = Path.GetDirectoryName(full)!;
            var name = Path.GetFileName(full);
            var root = Path.GetFullPath(dir);
            if (!(folder == Path.Combine(root, "old") || folder == Path.Combine(root, "new"))
                || !name.StartsWith("FRIDAY-", StringComparison.Ordinal) || !name.EndsWith("-x64.msi", StringComparison.Ordinal))
                return false;
            var sums = File.ReadAllBytes(Path.Combine(folder, "SHA256SUMS"));
            var sig = File.ReadAllBytes(Path.Combine(folder, "SHA256SUMS.sig"));
            return Updates.Signed(sums, sig, publicKey)
                && Updates.ExpectedSha(System.Text.Encoding.UTF8.GetString(sums), name) == Updates.Sha256(full);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return false;
        }
    }

    /// <summary>The name of the task (and the sign-in entry) that runs a revert.</summary>
    public const string TaskName = "FRIDAY revert";

    /// <summary>Takes the new version off and puts the old one back, each step waited for;
    /// with <paramref name="fallback"/> (the new MSI still checks out), the new one goes back
    /// on if the old one won't, so FRIDAY is never left off. Then FRIDAY starts again. Until
    /// it finishes, a sign-in entry runs it again (a revert cut short by a shutdown).</summary>
    public static string RevertScript(RollbackPlan plan, string trayExe, bool fallback = true)
    {
        var lines = new List<string>
        {
            "@echo off",
            $"start \"\" /wait msiexec /x \"{plan.NewMsi}\" /passive /norestart",
            $"start \"\" /wait msiexec /i \"{plan.OldMsi}\" /passive /norestart",
            "set rc=%errorlevel%",
        };
        if (fallback)
            lines.Add($"if not \"%rc%\"==\"0\" if not \"%rc%\"==\"3010\" start \"\" /wait msiexec /i \"{plan.NewMsi}\" /passive /norestart");
        lines.Add($"reg delete \"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce\" /v \"{TaskName}\" /f >nul 2>&1");
        lines.Add($"schtasks /delete /tn \"{TaskName}\" /f >nul 2>&1");
        lines.Add($"start \"\" \"{trayExe}\"");
        return string.Join("\r\n", lines) + "\r\n";
    }

    /// <summary>The task that runs the script: on battery too, never stopped for time or for
    /// unplugging (a revert stopped between its two installers leaves no FRIDAY at all).</summary>
    public static string TaskXml(string script) => $"""
        <?xml version="1.0" encoding="UTF-16"?>
        <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
          <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
          <Settings>
            <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
            <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
            <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
            <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
            <Enabled>true</Enabled>
          </Settings>
          <Actions Context="Author"><Exec><Command>{System.Security.SecurityElement.Escape(script)}</Command></Exec></Actions>
        </Task>
        """;
}
