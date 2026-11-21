using System.Text.Json;
using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class BrowserLinkTests
{
    [Fact]
    public void The_chromium_manifest_allows_only_fridays_extension()
    {
        var doc = JsonDocument.Parse(BrowserLink.Manifest(@"C:\Users\x\AppData\Local\Programs\FRIDAY\friday-ctl.exe", firefox: false)).RootElement;
        Assert.Equal("org.friday.browser", doc.GetProperty("name").GetString());
        Assert.Equal("stdio", doc.GetProperty("type").GetString());
        Assert.EndsWith("friday-ctl.exe", doc.GetProperty("path").GetString());
        var origins = doc.GetProperty("allowed_origins").EnumerateArray().Select(e => e.GetString()).ToList();
        Assert.Equal(["chrome-extension://eiblopljacafgajanhkngbhejpjjmfff/"], origins);
        Assert.False(doc.TryGetProperty("allowed_extensions", out _));
    }

    [Fact]
    public void The_firefox_manifest_names_the_extension_id()
    {
        var doc = JsonDocument.Parse(BrowserLink.Manifest("friday-ctl.exe", firefox: true)).RootElement;
        Assert.Equal("browser@friday-os", doc.GetProperty("allowed_extensions")[0].GetString());
        Assert.False(doc.TryGetProperty("allowed_origins", out _));
    }

    [Fact]
    public void Every_browser_says_where_its_hosts_and_extensions_page_are()
    {
        Assert.All(BrowserLink.All, b =>
        {
            Assert.NotEmpty(b.HostKeys);
            Assert.EndsWith(".exe", b.Exe);
            Assert.Contains(b.Firefox ? "manifest.json" : "Load unpacked", BrowserLink.Steps(b, @"C:\FRIDAY\browser-extension"));
        });
    }

    [Fact]
    public void The_page_goes_only_when_it_is_the_tab_in_front()
    {
        var tab = """{"title":"Rust book","url":"https://example.com/book","selection":"","text":"Ownership rules"}""";
        var ctx = BrowserLink.PageContext(tab, "Rust book - Google Chrome");
        Assert.Contains("Address: https://example.com/book", ctx);
        Assert.Contains("Ownership rules", ctx);
        Assert.Null(BrowserLink.PageContext(tab, "New Incognito tab - Google Chrome"));
        Assert.NotNull(BrowserLink.PageContext(tab, "Rust book \u2014 Mozilla Firefox"));
        // Only the window whose title starts with the tab's, not one that
        // merely contains it.
        Assert.Null(BrowserLink.PageContext(tab, "Notes on Rust book - Google Chrome"));
        Assert.Null(BrowserLink.PageContext(tab, "Rust bookshelf - Google Chrome"));
        Assert.Null(BrowserLink.PageContext("""{"error":"x"}""", "Rust book - Google Chrome"));
        Assert.Null(BrowserLink.PageContext("not json", "x"));
    }
}
