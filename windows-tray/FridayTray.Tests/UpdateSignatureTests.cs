using System.Security.Cryptography;
using System.Text;
using FridayTray.Core;
using Xunit;

public class UpdateSignatureTests
{
    // Made by `openssl dgst -sha256 -sign` with a throwaway key, exactly as
    // release.yml signs SHA256SUMS: the tray must accept openssl's format.
    private const string OpensslKey = """
-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEcdO1YNTfJY604xBpSVVT6+QqZ5ve
vb7dkR04vmS8p0YdxzB3hOLsV/n8Jk8vcF3R7ZhaoxX4Y2mLRyLDQBMmQg==
-----END PUBLIC KEY-----
""";
    private const string OpensslSig = "MEQCIEr+U20yB6oWrymrrwpSpB+ohcO4nWbHWuVHlBZObNHAAiBWQeCIwMxn/XXNKaTnCWJS20KMBksjAnxEeZlGUCRw+w==";
    private static readonly byte[] Sums = Encoding.UTF8.GetBytes("abc123  FRIDAY-0.9.0-x64.msi\n");

    [Fact]
    public void An_openssl_signature_over_the_checksums_is_accepted()
    {
        Assert.True(Updates.Signed(Sums, Convert.FromBase64String(OpensslSig), OpensslKey));
    }

    [Fact]
    public void Changed_checksums_a_missing_signature_or_another_key_are_refused()
    {
        var sig = Convert.FromBase64String(OpensslSig);
        var tampered = Encoding.UTF8.GetBytes("def456  FRIDAY-0.9.0-x64.msi\n");
        Assert.False(Updates.Signed(tampered, sig, OpensslKey));
        Assert.False(Updates.Signed(Sums, [], OpensslKey));
        // FRIDAY's real key didn't sign this.
        Assert.False(Updates.Signed(Sums, sig));
        using var other = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var otherSig = other.SignData(Sums, HashAlgorithmName.SHA256, DSASignatureFormat.Rfc3279DerSequence);
        Assert.False(Updates.Signed(Sums, otherSig, OpensslKey));
    }

    [Fact]
    public void The_built_in_key_is_the_one_in_the_repo()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "installer", "windows", "friday-update-key.pub")))
            dir = dir.Parent;
        Assert.NotNull(dir);
        var committed = File.ReadAllText(Path.Combine(dir!.FullName, "installer", "windows", "friday-update-key.pub"));
        Assert.Equal(committed.Trim(), Updates.UpdateKey.Trim());
    }
}
