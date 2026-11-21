using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class UpdatesTests
{
    private const string Latest = """
        {"tag_name": "v0.7.4", "assets": [
          {"name": "friday-git-0.7.4.r1-1-x86_64.pkg.tar.zst", "browser_download_url": "https://example.com/pkg"},
          {"name": "FRIDAY-0.7.4-x64.msi", "browser_download_url": "https://example.com/msi"},
          {"name": "SHA256SUMS", "browser_download_url": "https://example.com/sums"}
        ]}
        """;

    [Fact]
    public void A_release_gives_its_version_msi_and_checksums()
    {
        var r = Updates.Parse(Latest);
        Assert.Equal(new Release("0.7.4", "FRIDAY-0.7.4-x64.msi", "https://example.com/msi", "https://example.com/sums"), r);
    }

    [Fact]
    public void A_release_without_an_msi_or_checksums_is_not_offered()
    {
        Assert.Null(Updates.Parse("""{"tag_name": "v0.7.4", "assets": [{"name": "FRIDAY-0.7.4-x64.msi", "browser_download_url": "u"}]}"""));
        Assert.Null(Updates.Parse("""{"tag_name": "v0.7.4", "assets": [{"name": "SHA256SUMS", "browser_download_url": "u"}]}"""));
        Assert.Null(Updates.Parse("""{"message": "API rate limit exceeded"}"""));
        Assert.Null(Updates.Parse("not json"));
    }

    [Theory]
    [InlineData("0.7.4", "0.7.3", true)]
    [InlineData("0.10.0", "0.9.9", true)]
    [InlineData("0.7.3", "0.7.3", false)]
    [InlineData("0.7.2", "0.7.3", false)]
    [InlineData("0.8.0-alpha", "0.7.3", true)]
    [InlineData("garbage", "0.7.3", false)]
    public void Newer_compares_numbers_not_text(string latest, string current, bool newer) =>
        Assert.Equal(newer, Updates.Newer(latest, current));

    [Fact]
    public void The_expected_checksum_is_the_msis_line()
    {
        var sums = "aaa  friday-os-0.7.4-x86_64.iso\nBBB  FRIDAY-0.7.4-x64.msi\nccc *SHA-other\n";
        Assert.Equal("bbb", Updates.ExpectedSha(sums, "FRIDAY-0.7.4-x64.msi"));
        Assert.Null(Updates.ExpectedSha(sums, "FRIDAY-0.7.5-x64.msi"));
    }

    [Fact]
    public void Sha256_of_a_file()
    {
        var p = Path.GetTempFileName();
        File.WriteAllText(p, "abc");
        Assert.Equal("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", Updates.Sha256(p));
        File.Delete(p);
    }
}
