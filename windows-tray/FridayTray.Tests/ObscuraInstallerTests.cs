namespace FridayTray.Tests;

using System.IO.Compression;
using System.Net;
using System.Text;
using System.Text.Json;
using FridayTray.Core;
using Xunit;

public class ObscuraInstallerTests
{
    [Fact]
    public void Pinned_constants_match_tools_json()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "packaging", "tools", "tools.json")))
            dir = dir.Parent;

        Assert.NotNull(dir);
        var jsonText = File.ReadAllText(Path.Combine(dir!.FullName, "packaging", "tools", "tools.json"));
        using var doc = JsonDocument.Parse(jsonText);

        var obscura = doc.RootElement.GetProperty("obscura");
        var repo = obscura.GetProperty("repo").GetString();
        var version = obscura.GetProperty("version").GetString();
        var winAsset = obscura.GetProperty("assets").GetProperty("windows-x86_64");
        var assetName = winAsset.GetProperty("name").GetString();
        var sha256 = winAsset.GetProperty("sha256").GetString();

        Assert.Equal(ObscuraInstaller.Repo, repo);
        Assert.Equal(ObscuraInstaller.Version, version);
        Assert.Equal(ObscuraInstaller.AssetName, assetName);
        Assert.Equal(ObscuraInstaller.ExpectedSha256, sha256);
        Assert.Equal(ObscuraInstaller.DownloadUrl, $"https://github.com/{repo}/releases/download/v{version}/{assetName}");
    }

    [Fact]
    public void Target_paths_and_installed_status_work()
    {
        var tempLocal = Path.Combine(Path.GetTempPath(), "friday-test-local-" + Guid.NewGuid().ToString("N"));
        var tempApp = Path.Combine(Path.GetTempPath(), "friday-test-app-" + Guid.NewGuid().ToString("N"));
        try
        {
            var paths = new Paths(tempLocal, tempApp);
            Assert.Equal(Path.Combine(paths.StateDir, "obscura"), ObscuraInstaller.TargetDir(paths));
            Assert.Equal(Path.Combine(paths.StateDir, "obscura", "obscura.exe"), ObscuraInstaller.TargetExe(paths));
            Assert.False(ObscuraInstaller.IsInstalled(paths));

            Directory.CreateDirectory(ObscuraInstaller.TargetDir(paths));
            File.WriteAllText(ObscuraInstaller.TargetExe(paths), "dummy");
            Assert.True(ObscuraInstaller.IsInstalled(paths));
        }
        finally
        {
            try { Directory.Delete(tempLocal, true); } catch { }
            try { Directory.Delete(tempApp, true); } catch { }
        }
    }

    [Fact]
    public void Compute_and_verify_sha256()
    {
        var content = "Hello Friday Obscura";
        var bytes = Encoding.UTF8.GetBytes(content);
        var expectedHash = "3a6101a1989395227e0ef648602f96a2d503322f2a29c0204ab77df8b601eff6";

        var actualHash = ObscuraInstaller.ComputeSha256(bytes);
        Assert.Equal(expectedHash, actualHash);
        Assert.True(ObscuraInstaller.VerifySha256(bytes, expectedHash));
        Assert.False(ObscuraInstaller.VerifySha256(bytes, "0000000000000000000000000000000000000000000000000000000000000000"));

        var tempFile = Path.GetTempFileName();
        try
        {
            File.WriteAllBytes(tempFile, bytes);
            Assert.True(ObscuraInstaller.VerifySha256(tempFile, expectedHash));
            Assert.False(ObscuraInstaller.VerifySha256(tempFile, "badhash"));
        }
        finally
        {
            File.Delete(tempFile);
        }
    }

    [Fact]
    public void PickZipEntry_finds_exe_and_ExtractExe_writes_it()
    {
        using var ms = new MemoryStream();
        using (var archive = new ZipArchive(ms, ZipArchiveMode.Create, leaveOpen: true))
        {
            var other = archive.CreateEntry("README.txt");
            using (var w = new StreamWriter(other.Open()))
                w.Write("instructions");

            var exe = archive.CreateEntry("obscura.exe");
            using (var w = new StreamWriter(exe.Open()))
                w.Write("fake-binary-content");
        }

        ms.Position = 0;
        using (var readArchive = new ZipArchive(ms, ZipArchiveMode.Read, leaveOpen: true))
        {
            var entry = ObscuraInstaller.PickZipEntry(readArchive);
            Assert.NotNull(entry);
            Assert.Equal("obscura.exe", entry!.Name);
        }

        ms.Position = 0;
        var tempDir = Path.Combine(Path.GetTempPath(), "obscura-extract-" + Guid.NewGuid().ToString("N"));
        var targetExe = Path.Combine(tempDir, "obscura.exe");
        try
        {
            ObscuraInstaller.ExtractExe(ms, targetExe);
            Assert.True(File.Exists(targetExe));
            Assert.Equal("fake-binary-content", File.ReadAllText(targetExe));
        }
        finally
        {
            try { Directory.Delete(tempDir, true); } catch { }
        }
    }

    [Fact]
    public async Task InstallAsync_refuses_and_cleans_up_on_checksum_mismatch()
    {
        var tempLocal = Path.Combine(Path.GetTempPath(), "friday-test-local-" + Guid.NewGuid().ToString("N"));
        var tempApp = Path.Combine(Path.GetTempPath(), "friday-test-app-" + Guid.NewGuid().ToString("N"));
        var paths = new Paths(tempLocal, tempApp);

        // Handler that serves dummy data that does NOT match ObscuraInstaller.ExpectedSha256
        var handler = new FakeHttpHandler(new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new ByteArrayContent(Encoding.UTF8.GetBytes("tampered-or-wrong-payload"))
        });
        using var client = new HttpClient(handler);

        try
        {
            var (ok, error) = await ObscuraInstaller.InstallAsync(paths, client);
            Assert.False(ok);
            Assert.NotNull(error);
            Assert.Contains("checksum mismatch", error);
            Assert.False(ObscuraInstaller.IsInstalled(paths));
        }
        finally
        {
            try { Directory.Delete(tempLocal, true); } catch { }
            try { Directory.Delete(tempApp, true); } catch { }
        }
    }

    private class FakeHttpHandler : HttpMessageHandler
    {
        private readonly HttpResponseMessage _response;
        public FakeHttpHandler(HttpResponseMessage response) => _response = response;

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(_response);
    }
}
