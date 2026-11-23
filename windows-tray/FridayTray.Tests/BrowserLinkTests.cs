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
        Assert.Equal(["chrome-extension://eiblopljacafgajanhkngbhejpjjmfff/", "chrome-extension://ncbhacgfkbbpmocppmagejakdnkikcpk/"], origins);
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

    [Fact]
    public void Extensions_window_title_matches_expected_patterns()
    {
        Assert.True(BrowserLink.IsExtensionsWindow("Extensions - Google Chrome"));
        Assert.True(BrowserLink.IsExtensionsWindow("Extensions - Personal - Microsoft Edge"));
        Assert.True(BrowserLink.IsExtensionsWindow("Extensions - Brave"));
        Assert.True(BrowserLink.IsExtensionsWindow("Extensions - Vivaldi"));
        Assert.True(BrowserLink.IsExtensionsWindow("Extensions"));
        Assert.True(BrowserLink.IsExtensionsWindow("extensions"));
        Assert.False(BrowserLink.IsExtensionsWindow("Google Chrome"));
        Assert.False(BrowserLink.IsExtensionsWindow("New Tab - Google Chrome"));
        Assert.False(BrowserLink.IsExtensionsWindow(""));
        Assert.False(BrowserLink.IsExtensionsWindow(null));
    }

    [Fact]
    public void Folder_picker_dialog_matches_title_or_class()
    {
        Assert.True(BrowserLink.IsFolderPickerDialog("Select the extension directory"));
        Assert.True(BrowserLink.IsFolderPickerDialog("Select the extension directory - Google Chrome"));
        Assert.True(BrowserLink.IsFolderPickerDialog("Open", "#32770"));
        Assert.True(BrowserLink.IsFolderPickerDialog("Select Folder", "#32770"));
        Assert.False(BrowserLink.IsFolderPickerDialog("Save As", "Chrome_WidgetWin_1"));
        Assert.False(BrowserLink.IsFolderPickerDialog(null, null));
        Assert.False(BrowserLink.IsFolderPickerDialog("", ""));
    }

    [Fact]
    public void Failure_messages_guide_manual_steps_and_mention_clipboard()
    {
        var chrome = BrowserLink.All[0];
        foreach (var step in Enum.GetValues<ExtensionInstallStep>())
        {
            var msg = BrowserLink.FailureMessage(step, chrome);
            Assert.False(string.IsNullOrWhiteSpace(msg));
            if (step != ExtensionInstallStep.Verify)
                Assert.Contains("clipboard", msg);
        }

        Assert.Equal(
            "Couldn't find Load unpacked; the folder is on the clipboard: click Load unpacked and paste it.",
            BrowserLink.FailureMessage(ExtensionInstallStep.LoadUnpacked));
        Assert.Contains("Google Chrome", BrowserLink.FailureMessage(ExtensionInstallStep.Verify, chrome));
    }

    [Fact]
    public void Success_message_names_browser()
    {
        var chrome = BrowserLink.All[0];
        Assert.Equal("FRIDAY's extension is on in Google Chrome. If it asks to turn off developer-mode extensions, choose Not now.", BrowserLink.SuccessMessage(chrome));
        Assert.StartsWith("FRIDAY's extension is on in Brave.", BrowserLink.SuccessMessage("Brave"));
    }

    [Fact]
    public void Extension_autoinstall_feature_is_available_and_on_by_default()
    {
        Assert.Contains(AppSettings.Features, f => f.Name == "extension-autoinstall"
            && f.What == "Settings installs the browser extension for you (Developer mode, Load unpacked)");
        Assert.True(AppSettings.ExtensionAutoinstallOn(new Dictionary<string, string>()));
        Assert.False(AppSettings.ExtensionAutoinstallOn(new Dictionary<string, string> { ["FRIDAY_FEATURES_OFF"] = "extension-autoinstall" }));
    }
}
