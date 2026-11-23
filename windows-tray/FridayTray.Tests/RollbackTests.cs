using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class RollbackTests
{
    private static readonly DateTime At = new(2026, 11, 23, 9, 0, 0, DateTimeKind.Utc);
    private static readonly RollbackPlan Plan = new("0.8.2", "0.8.3", @"C:\r\old\FRIDAY-0.8.2-x64.msi", @"C:\r\new\FRIDAY-0.8.3-x64.msi", At);

    [Fact]
    public void The_new_version_waits_for_its_daemon_then_keeps_or_reverts()
    {
        Assert.Equal(RollbackStep.Wait, Rollback.Decide(Plan, "0.8.3", null, At.AddMinutes(1)));
        Assert.Equal(RollbackStep.Clear, Rollback.Decide(Plan, "0.8.3", true, At.AddMinutes(1)));
        Assert.Equal(RollbackStep.Revert, Rollback.Decide(Plan, "0.8.3", false, At.AddMinutes(3)));
    }

    [Fact]
    public void Back_on_the_old_version_it_says_so_once_and_never_loops()
    {
        var reverted = Plan with { Reverted = true };
        Assert.Equal(RollbackStep.TellReverted, Rollback.Decide(reverted, "0.8.2", null, At.AddMinutes(5)));
        // A revert that left the new version on (the old MSI failed) doesn't try again.
        Assert.Equal(RollbackStep.Clear, Rollback.Decide(reverted, "0.8.3", false, At.AddMinutes(5)));
    }

    [Fact]
    public void An_update_that_never_ran_is_forgotten_after_a_day()
    {
        Assert.Equal(RollbackStep.None, Rollback.Decide(null, "0.8.2", null, At));
        Assert.Equal(RollbackStep.None, Rollback.Decide(Plan, "0.8.2", null, At.AddHours(2)));
        Assert.Equal(RollbackStep.Clear, Rollback.Decide(Plan, "0.8.2", null, At.AddDays(2)));
        Assert.Equal(RollbackStep.Clear, Rollback.Decide(Plan, "0.9.0", null, At));
    }

    [Fact]
    public void The_plan_survives_a_restart_and_the_script_swaps_the_versions()
    {
        var file = Path.Combine(Path.GetTempPath(), "friday-rollback-" + Guid.NewGuid().ToString("N"), "plan.json");
        try
        {
            Rollback.Write(file, Plan);
            Assert.Equal(Plan, Rollback.Read(file));
            File.WriteAllText(file, "{not json");
            Assert.Null(Rollback.Read(file));
        }
        finally
        {
            Directory.Delete(Path.GetDirectoryName(file)!, recursive: true);
        }
        var script = Rollback.RevertScript(Plan, @"C:\f\friday-tray.exe");
        Assert.Contains("start \"\" /wait msiexec /x \"C:\\r\\new\\FRIDAY-0.8.3-x64.msi\" /passive /norestart\r\n", script);
        Assert.Contains("start \"\" /wait msiexec /i \"C:\\r\\old\\FRIDAY-0.8.2-x64.msi\" /passive /norestart\r\n", script);
        // The old one failing puts the new one back; FRIDAY starts again either way.
        Assert.Contains("if not \"%rc%\"==\"0\" if not \"%rc%\"==\"3010\" start \"\" /wait msiexec /i \"C:\\r\\new\\FRIDAY-0.8.3-x64.msi\"", script);
        Assert.Contains("start \"\" \"C:\\f\\friday-tray.exe\"", script);
        Assert.Equal("https://api.github.com/repos/sreevarshan-xenoz/project-friday/releases/tags/v0.8.2", Updates.TagUrl("0.8.2"));
    }

    [Fact]
    public void Pre_release_tags_count_as_their_version()
    {
        var alpha = Plan with { To = "0.8.3-alpha" };
        Assert.Equal(RollbackStep.Wait, Rollback.Decide(alpha, "0.8.3", null, At));
    }

    [Fact]
    public void Only_a_kept_msi_its_signed_sums_still_vouch_for_is_run()
    {
        using var key = System.Security.Cryptography.ECDsa.Create(System.Security.Cryptography.ECCurve.NamedCurves.nistP256);
        var pub = key.ExportSubjectPublicKeyInfoPem();
        var dir = Path.Combine(Path.GetTempPath(), "friday-rollback-" + Guid.NewGuid().ToString("N"));
        var old = Path.Combine(dir, "old");
        Directory.CreateDirectory(old);
        try
        {
            var msi = Path.Combine(old, "FRIDAY-0.8.2-x64.msi");
            File.WriteAllText(msi, "the old installer");
            var sums = System.Text.Encoding.UTF8.GetBytes($"{Updates.Sha256(msi)}  FRIDAY-0.8.2-x64.msi\n");
            File.WriteAllBytes(Path.Combine(old, "SHA256SUMS"), sums);
            File.WriteAllBytes(Path.Combine(old, "SHA256SUMS.sig"),
                key.SignData(sums, System.Security.Cryptography.HashAlgorithmName.SHA256, System.Security.Cryptography.DSASignatureFormat.Rfc3279DerSequence));
            Assert.True(Rollback.Verified(dir, msi, pub));
            // Not FRIDAY's key, a file outside the folder, or a changed MSI: never run.
            Assert.False(Rollback.Verified(dir, msi));
            var elsewhere = Path.Combine(dir, "FRIDAY-0.8.2-x64.msi");
            File.Copy(msi, elsewhere);
            Assert.False(Rollback.Verified(dir, elsewhere, pub));
            File.WriteAllText(msi, "something else");
            Assert.False(Rollback.Verified(dir, msi, pub));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }
}
