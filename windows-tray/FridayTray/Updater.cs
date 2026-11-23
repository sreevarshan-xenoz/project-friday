using System.Diagnostics;
using System.Net.Http;
using FridayTray.Core;

namespace FridayTray;

/// <summary>The network half of <see cref="Updates"/>: asks GitHub, downloads, checks, runs the MSI.</summary>
internal static class Updater
{
    public static string Current
    {
        get
        {
            var v = typeof(Updater).Assembly.GetName().Version;
            return $"{v?.Major}.{v?.Minor}.{v?.Build}";
        }
    }

    private static HttpClient Http(TimeSpan timeout)
    {
        var http = new HttpClient { Timeout = timeout };
        http.DefaultRequestHeaders.UserAgent.ParseAdd("FRIDAY-tray/" + Current);
        return http;
    }

    /// <summary>The newest release when it is newer than this one; null when it isn't or GitHub can't be reached.</summary>
    public static async Task<Release?> NewerAsync()
    {
        try
        {
            using var http = Http(TimeSpan.FromSeconds(15));
            var r = Updates.Parse(await http.GetStringAsync(Updates.LatestUrl));
            return r is not null && Updates.Newer(r.Version, Current) ? r : null;
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException)
        {
            return null;
        }
    }

    /// <summary>Downloads the MSI, checks it against SHA256SUMS and opens it. With
    /// <paramref name="keepIn"/>, the MSI of the version installed now is kept first, so a
    /// bad update can revert itself (<see cref="Rollback"/>). Returns what went wrong, or null.</summary>
    public static async Task<string?> InstallAsync(Release r, Paths? keepIn = null)
    {
        try
        {
            using var http = Http(TimeSpan.FromMinutes(10));
            // A folder of its own each time: nothing else can have put a file there.
            var dir = Path.Combine(Path.GetTempPath(), "friday-update-" + Guid.NewGuid().ToString("N"));
            var (path, problem) = await FetchAsync(http, r, dir);
            if (path is null)
                return problem;
            if (keepIn is not null)
                await KeepForRollbackAsync(http, keepIn, r, path);
            Process.Start(new ProcessStartInfo("msiexec.exe", $"/i \"{path}\"") { UseShellExecute = true });
            return null;
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            return e.Message;
        }
    }

    /// <summary>The release's MSI, downloaded into <paramref name="dir"/> and checked
    /// against its signed SHA256SUMS; or null and what went wrong.</summary>
    private static async Task<(string? Path, string? Problem)> FetchAsync(HttpClient http, Release r, string dir)
    {
        var sums = await http.GetByteArrayAsync(r.SumsUrl);
        byte[] sig;
        try { sig = await http.GetByteArrayAsync(r.SigUrl); }
        catch (HttpRequestException) { sig = []; }
        if (!Updates.Signed(sums, sig))
            return (null, "This release isn't signed with FRIDAY's update key, so it wasn't downloaded.");
        var expected = Updates.ExpectedSha(System.Text.Encoding.UTF8.GetString(sums), r.MsiName);
        if (expected is null)
            return (null, $"The release's checksum file doesn't list {r.MsiName}.");
        Directory.CreateDirectory(dir);
        // Kept beside it, so a kept MSI can be checked again later (Rollback.Verified).
        await File.WriteAllBytesAsync(Path.Combine(dir, "SHA256SUMS"), sums);
        await File.WriteAllBytesAsync(Path.Combine(dir, "SHA256SUMS.sig"), sig);
        var path = Path.Combine(dir, r.MsiName);
        await using (var body = await http.GetStreamAsync(r.MsiUrl))
        await using (var file = File.Create(path))
            await body.CopyToAsync(file);
        if (Updates.Sha256(path) != expected)
        {
            File.Delete(path);
            return (null, "The download doesn't match the release's checksum, so it wasn't opened.");
        }
        return (path, null);
    }

    /// <summary>Keeps this version's MSI and the new one, and the plan that says which is
    /// which. Without this version's MSI (a build that was never released, GitHub
    /// unreachable) the update goes ahead with nothing to go back to.</summary>
    private static async Task KeepForRollbackAsync(HttpClient http, Paths paths, Release next, string nextMsi)
    {
        // Gathered beside the folder and swapped in whole: a failed download
        // leaves the plan that was there as it was.
        var dir = paths.RollbackDir + ".next";
        try
        {
            if (Directory.Exists(dir))
                Directory.Delete(dir, recursive: true);
            var now = Updates.Parse(await http.GetStringAsync(Updates.TagUrl(Current)));
            if (now is null || Rollback.Plain(now.Version) != Current)
                return;
            var (oldMsi, _) = await FetchAsync(http, now, Path.Combine(dir, "old"));
            if (oldMsi is null)
                return;
            var newDir = Path.Combine(dir, "new");
            Directory.CreateDirectory(newDir);
            foreach (var f in new[] { nextMsi, Path.Combine(Path.GetDirectoryName(nextMsi)!, "SHA256SUMS"), Path.Combine(Path.GetDirectoryName(nextMsi)!, "SHA256SUMS.sig") })
                File.Copy(f, Path.Combine(newDir, Path.GetFileName(f)), overwrite: true);
            if (Directory.Exists(paths.RollbackDir))
                Directory.Delete(paths.RollbackDir, recursive: true);
            // A virus scanner often still holds a file it just read: try again for a bit.
            for (var i = 0; ; i++)
            {
                try
                {
                    Directory.Move(dir, paths.RollbackDir);
                    break;
                }
                catch (Exception e) when (i < 10 && e is IOException or UnauthorizedAccessException)
                {
                    await Task.Delay(1000);
                }
            }
            Rollback.Write(paths.RollbackFile, new RollbackPlan(Current, Rollback.Plain(next.Version),
                Path.Combine(paths.RollbackDir, "old", now.MsiName), Path.Combine(paths.RollbackDir, "new", next.MsiName), DateTime.UtcNow));
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException or UnauthorizedAccessException)
        {
            // Nothing to go back to: the update itself still goes ahead.
        }
    }
}
