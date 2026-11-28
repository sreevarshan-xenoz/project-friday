namespace FridayTray.Core;

using System.IO.Compression;
using System.Security.Cryptography;

public static class ObscuraInstaller
{
    public const string Version = "0.2.7";
    public const string Repo = "h4ckf0r0day/obscura";
    public const string AssetName = "obscura-x86_64-windows-no-render.zip";
    public const string ExpectedSha256 = "a2ac7d8b08e93e3ed31f6f14cc7057d822725d3f55d91fc7992a6f2d3e0ad1b6";
    public const string DownloadUrl = $"https://github.com/{Repo}/releases/download/v{Version}/{AssetName}";
    public const string ExeName = "obscura.exe";

    public static string TargetDir(Paths paths) => paths.ObscuraDir;
    public static string TargetExe(Paths paths) => paths.ObscuraExe;

    public static bool IsInstalled(Paths paths) => File.Exists(TargetExe(paths));

    public static string ComputeSha256(byte[] data)
    {
        var hash = SHA256.HashData(data);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    public static string ComputeSha256(Stream stream)
    {
        var hash = SHA256.HashData(stream);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    public static string ComputeSha256(string filePath)
    {
        using var stream = File.OpenRead(filePath);
        return ComputeSha256(stream);
    }

    public static bool VerifySha256(string filePath, string expectedSha256)
    {
        if (!File.Exists(filePath))
            return false;
        var actual = ComputeSha256(filePath);
        return string.Equals(actual, expectedSha256, StringComparison.OrdinalIgnoreCase);
    }

    public static bool VerifySha256(byte[] data, string expectedSha256)
    {
        var actual = ComputeSha256(data);
        return string.Equals(actual, expectedSha256, StringComparison.OrdinalIgnoreCase);
    }

    public static ZipArchiveEntry? PickZipEntry(ZipArchive archive, string exeName = ExeName)
    {
        return archive.Entries.FirstOrDefault(e =>
            string.Equals(Path.GetFileName(e.FullName), exeName, StringComparison.OrdinalIgnoreCase));
    }

    public static void ExtractExe(Stream zipStream, string targetExePath, string exeName = ExeName)
    {
        using var archive = new ZipArchive(zipStream, ZipArchiveMode.Read, leaveOpen: true);
        var entry = PickZipEntry(archive, exeName)
            ?? throw new InvalidOperationException($"'{exeName}' was not found in the zip archive.");

        var dir = Path.GetDirectoryName(targetExePath);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);

        var tempTarget = targetExePath + ".tmp";
        try
        {
            entry.ExtractToFile(tempTarget, overwrite: true);
            File.Move(tempTarget, targetExePath, overwrite: true);
        }
        finally
        {
            if (File.Exists(tempTarget))
            {
                try { File.Delete(tempTarget); } catch { }
            }
        }
    }

    public static async Task<(bool Ok, string? Error)> InstallAsync(Paths paths, HttpClient? client = null, CancellationToken cancel = default)
    {
        var ownClient = client == null;
        var http = client ?? new HttpClient { Timeout = TimeSpan.FromMinutes(5) };
        var tempZip = Path.Combine(Path.GetTempPath(), $"obscura-download-{Guid.NewGuid():N}.zip");
        try
        {
            using (var response = await http.GetAsync(DownloadUrl, HttpCompletionOption.ResponseHeadersRead, cancel))
            {
                response.EnsureSuccessStatusCode();
                await using (var fs = new FileStream(tempZip, FileMode.Create, FileAccess.Write, FileShare.None))
                {
                    await response.Content.CopyToAsync(fs, cancel);
                }
            }

            var actualSha = ComputeSha256(tempZip);
            if (!string.Equals(actualSha, ExpectedSha256, StringComparison.OrdinalIgnoreCase))
            {
                try { File.Delete(tempZip); } catch { }
                return (false, $"Downloaded archive checksum mismatch: expected {ExpectedSha256}, got {actualSha}. Download was deleted.");
            }

            await using (var zipStream = File.OpenRead(tempZip))
            {
                ExtractExe(zipStream, TargetExe(paths), ExeName);
            }

            return (true, null);
        }
        catch (Exception ex)
        {
            return (false, ex.Message);
        }
        finally
        {
            if (File.Exists(tempZip))
            {
                try { File.Delete(tempZip); } catch { }
            }
            if (ownClient)
                http.Dispose();
        }
    }
}
