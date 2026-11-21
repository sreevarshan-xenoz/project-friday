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

    /// <summary>Downloads the MSI, checks it against SHA256SUMS and opens it. Returns what went wrong, or null.</summary>
    public static async Task<string?> InstallAsync(Release r)
    {
        try
        {
            using var http = Http(TimeSpan.FromMinutes(10));
            var expected = Updates.ExpectedSha(await http.GetStringAsync(r.SumsUrl), r.MsiName);
            if (expected is null)
                return $"The release's checksum file doesn't list {r.MsiName}.";
            var dir = Path.Combine(Path.GetTempPath(), "friday-update");
            Directory.CreateDirectory(dir);
            var path = Path.Combine(dir, r.MsiName);
            await using (var body = await http.GetStreamAsync(r.MsiUrl))
            await using (var file = File.Create(path))
                await body.CopyToAsync(file);
            if (Updates.Sha256(path) != expected)
            {
                File.Delete(path);
                return "The download doesn't match the release's checksum, so it wasn't opened.";
            }
            Process.Start(new ProcessStartInfo("msiexec.exe", $"/i \"{path}\"") { UseShellExecute = true });
            return null;
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            return e.Message;
        }
    }
}
