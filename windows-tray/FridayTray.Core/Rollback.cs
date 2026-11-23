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

    /// <summary>What to do now, running <paramref name="current"/>; <paramref name="healthy"/>
    /// is null until the daemon has come up or the grace time ran out.</summary>
    public static RollbackStep Decide(RollbackPlan? plan, string current, bool? healthy, DateTime now)
    {
        if (plan is null)
            return RollbackStep.None;
        if (current == plan.From)
            // Reverted: say so. Otherwise the update hasn't run (yet): a day
            // later it never will (the installer was cancelled).
            return plan.Reverted ? RollbackStep.TellReverted
                : now - plan.At > TimeSpan.FromDays(1) ? RollbackStep.Clear : RollbackStep.None;
        if (current != plan.To || plan.Reverted)
            return RollbackStep.Clear;
        return healthy switch
        {
            true => RollbackStep.Clear,
            false => RollbackStep.Revert,
            null => RollbackStep.Wait,
        };
    }

    /// <summary>The commands that take the new version off and put the old one back.</summary>
    public static string RevertScript(RollbackPlan plan) =>
        $"msiexec /x \"{plan.NewMsi}\" /passive\r\nmsiexec /i \"{plan.OldMsi}\" /passive\r\n";
}
