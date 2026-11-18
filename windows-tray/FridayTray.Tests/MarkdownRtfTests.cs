using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public sealed class MarkdownRtfTests
{
    private static string Rtf(string md) => MarkdownRtf.Convert(md, "Segoe UI", 10.5f, (236, 232, 226), (95, 211, 188));

    [Fact]
    public void Bold_Italic_Code_AndLinks_BecomeFormatting_NotMarks()
    {
        var r = Rtf("Your disk is mostly **photos** and *some* `cache`; see [the docs](https://example.com).");
        Assert.Contains(@"{\b photos}", r);
        Assert.Contains(@"{\i some}", r);
        Assert.Contains(@"{\f1\cf2 cache}", r);
        Assert.Contains("see the docs.", r);
        Assert.DoesNotContain("**", r);
        Assert.DoesNotContain("https://example.com", r);
    }

    [Fact]
    public void Lists_Headings_AndCodeBlocks()
    {
        var r = Rtf("# Disk\n- Pictures: 41 GB\n2. Empty Downloads\n```\nrm -rf ~/x\n```");
        Assert.Contains(@"{\b Disk}\par", r);
        Assert.Contains(@"\u8226? Pictures: 41 GB", r);
        Assert.Contains("2. Empty Downloads", r);
        Assert.Contains(@"{\f1\cf2 rm -rf ~/x}\par", r);
        Assert.DoesNotContain("```", r);
    }

    [Fact]
    public void Braces_Backslashes_AndNonAscii_AreEscaped()
    {
        var r = Rtf(@"C:\Users {x} café 5 × 3");
        Assert.Contains(@"C:\\Users \{x\}", r);
        Assert.Contains(@"caf\u233?", r);
        Assert.Contains(@"\u215?", r);
        Assert.StartsWith(@"{\rtf1", r);
        Assert.EndsWith("}", r);
    }

    [Fact]
    public void Underscores_InsideWords_And_Arithmetic_StayAsWritten()
    {
        var r = Rtf("snake_case_name and 2 * 3 * 4");
        Assert.Contains("snake_case_name", r);
        Assert.Contains("2 * 3 * 4", r);
    }
}
