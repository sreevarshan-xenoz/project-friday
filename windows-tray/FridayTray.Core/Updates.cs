using System.Security.Cryptography;
using System.Text.Json;

namespace FridayTray.Core;

/// <summary>A published FRIDAY release: its version, its MSI and the checksum file beside it.</summary>
public sealed record Release(string Version, string MsiName, string MsiUrl, string SumsUrl);

/// <summary>
/// Updating FRIDAY on Windows: the newest GitHub release is read from the
/// releases API, its MSI is downloaded, checked against the release's
/// SHA256SUMS and run with its normal installer window (the MSI closes the
/// running tray and starts the new one when it finishes).
/// </summary>
public static class Updates
{
    public const string LatestUrl = "https://api.github.com/repos/sreevarshan-xenoz/project-friday/releases/latest";

    /// <summary>The release in a releases-API answer, or null when it has no MSI or no SHA256SUMS.</summary>
    public static Release? Parse(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            var tag = root.GetProperty("tag_name").GetString() ?? "";
            string? msiName = null, msiUrl = null, sumsUrl = null;
            foreach (var asset in root.GetProperty("assets").EnumerateArray())
            {
                var name = asset.GetProperty("name").GetString() ?? "";
                var url = asset.GetProperty("browser_download_url").GetString() ?? "";
                if (name == "SHA256SUMS")
                    sumsUrl = url;
                else if (name.StartsWith("FRIDAY-", StringComparison.Ordinal) && name.EndsWith("-x64.msi", StringComparison.Ordinal))
                    (msiName, msiUrl) = (name, url);
            }
            if (msiName is null || msiUrl is null || sumsUrl is null)
                return null;
            return new Release(tag.TrimStart('v'), msiName, msiUrl, sumsUrl);
        }
        catch (Exception e) when (e is JsonException or KeyNotFoundException or InvalidOperationException)
        {
            return null;
        }
    }

    /// <summary>True when <paramref name="latest"/> is a higher x.y.z than <paramref name="current"/>.</summary>
    public static bool Newer(string latest, string current) =>
        System.Version.TryParse(latest.Split('-')[0], out var l)
        && System.Version.TryParse(current, out var c)
        && l > c;

    /// <summary>The checksum SHA256SUMS lists for a file, lower case, or null.</summary>
    public static string? ExpectedSha(string sums, string fileName)
    {
        foreach (var line in sums.Split('\n'))
        {
            var parts = line.Trim().Split(' ', 2, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length == 2 && parts[1].TrimStart('*', ' ') == fileName)
                return parts[0].ToLowerInvariant();
        }
        return null;
    }

    public static string Sha256(string path)
    {
        using var f = File.OpenRead(path);
        return Convert.ToHexString(SHA256.HashData(f)).ToLowerInvariant();
    }
}
