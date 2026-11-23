using System.Text.Json;

namespace FridayTray.Core;

/// <summary>A browser FRIDAY's extension can be added to.</summary>
/// <param name="HostKeys">Where (under HKCU) it looks for native-messaging hosts.</param>
public sealed record KnownBrowser(string Name, string Exe, string[] HostKeys, string ExtensionsPage, bool Firefox);

/// <summary>The steps in automating browser extension installation.</summary>
public enum ExtensionInstallStep
{
    Window,
    DeveloperMode,
    LoadUnpacked,
    FolderDialog,
    FolderInput,
    SelectFolder,
    Verify,
}

/// <summary>
/// FRIDAY's browser extension on Windows: the browser starts friday-ctl.exe
/// as its native-messaging host (no port; the extension asks only when FRIDAY
/// asks, and never in private windows). These are the browsers it knows and
/// the manifest each one reads.
/// </summary>
public static class BrowserLink
{
    public const string HostName = "org.friday.browser";
    public const string ExtensionId = "eiblopljacafgajanhkngbhejpjjmfff";
    public const string FirefoxId = "browser@friday-os";

    public static readonly KnownBrowser[] All =
    [
        new("Google Chrome", "chrome.exe", [@"Software\Google\Chrome\NativeMessagingHosts"], "chrome://extensions", false),
        new("Microsoft Edge", "msedge.exe", [@"Software\Microsoft\Edge\NativeMessagingHosts"], "edge://extensions", false),
        new("Brave", "brave.exe", [@"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts"], "brave://extensions", false),
        new("Vivaldi", "vivaldi.exe", [@"Software\Vivaldi\NativeMessagingHosts", @"Software\Chromium\NativeMessagingHosts"], "vivaldi://extensions", false),
        new("Firefox", "firefox.exe", [@"Software\Mozilla\NativeMessagingHosts"], "about:debugging#/runtime/this-firefox", true),
    ];

    /// <summary>The host manifest: friday-ctl.exe, allowed to talk only to FRIDAY's extension.</summary>
    public static string Manifest(string hostExe, bool firefox)
    {
        var m = new Dictionary<string, object>
        {
            ["name"] = HostName,
            ["description"] = "FRIDAY",
            ["path"] = hostExe,
            ["type"] = "stdio",
        };
        if (firefox)
            m["allowed_extensions"] = new[] { FirefoxId };
        else
            m["allowed_origins"] = new[] { $"chrome-extension://{ExtensionId}/" };
        return JsonSerializer.Serialize(m, new JsonSerializerOptions { WriteIndented = true });
    }

    /// <summary>What to do in the browser once, until the extension is in the browsers' stores.</summary>
    public static string Steps(KnownBrowser b, string extensionDir) => b.Firefox
        ? $"In Firefox's page that just opened, click \"Load Temporary Add-on\" and pick manifest.json in {extensionDir} (Firefox forgets it on restart until the extension is signed)."
        : $"In {b.Name}'s extensions page that just opened, turn on Developer mode, click \"Load unpacked\" and pick {extensionDir} (its path is on your clipboard).";

    /// <summary>Whether a window title is a browser's extensions page.</summary>
    public static bool IsExtensionsWindow(string? windowTitle) =>
        !string.IsNullOrWhiteSpace(windowTitle)
        && windowTitle.Contains("Extensions", StringComparison.OrdinalIgnoreCase);

    /// <summary>Whether a dialog title or class name is the folder picker for unpacked extensions.</summary>
    public static bool IsFolderPickerDialog(string? title, string? className = null) =>
        (!string.IsNullOrWhiteSpace(title) && title.Contains("Select the extension directory", StringComparison.OrdinalIgnoreCase))
        || string.Equals(className, "#32770", StringComparison.OrdinalIgnoreCase);

    /// <summary>One-line message on successful extension installation.</summary>
    public static string SuccessMessage(KnownBrowser b) => SuccessMessage(b.Name);

    /// <summary>One-line message on successful extension installation.</summary>
    public static string SuccessMessage(string browserName) =>
        $"FRIDAY's extension is on in {browserName}. If it asks to turn off developer-mode extensions, choose Not now.";

    /// <summary>Plain-English fallback message when a step in extension installation cannot be completed automatically.</summary>
    public static string FailureMessage(ExtensionInstallStep step, KnownBrowser b) => FailureMessage(step, b.Name);

    /// <summary>Plain-English fallback message when a step in extension installation cannot be completed automatically.</summary>
    public static string FailureMessage(ExtensionInstallStep step, string? browserName = null) => step switch
    {
        ExtensionInstallStep.Window =>
            "Couldn't find the extensions page; the folder is on the clipboard: turn on Developer mode, click Load unpacked and paste it.",
        ExtensionInstallStep.DeveloperMode =>
            "Couldn't find Developer mode; the folder is on the clipboard: turn on Developer mode, click Load unpacked and paste it.",
        ExtensionInstallStep.LoadUnpacked =>
            "Couldn't find Load unpacked; the folder is on the clipboard: click Load unpacked and paste it.",
        ExtensionInstallStep.FolderDialog =>
            "Couldn't find the folder dialog; the folder is on the clipboard: click Load unpacked and paste it.",
        ExtensionInstallStep.FolderInput =>
            "Couldn't set the folder path; the folder is on the clipboard: paste it into Folder: and click Select Folder.",
        ExtensionInstallStep.SelectFolder =>
            "Couldn't click Select Folder; the folder is on the clipboard: click Select Folder in the dialog.",
        ExtensionInstallStep.Verify =>
            string.IsNullOrEmpty(browserName)
                ? "Couldn't verify FRIDAY in extensions; check the extensions page to make sure it loaded."
                : $"Couldn't verify FRIDAY in {browserName}'s extensions; check the extensions page to make sure it loaded.",
        _ => "Extension installation didn't finish; the folder is on the clipboard.",
    };

    /// <summary>
    /// The page text to send instead of a screenshot, from `friday-ctl browser
    /// tab`'s answer, or null: no text, or a tab that isn't the one in the
    /// window in front (a private window the extension can't see).
    /// </summary>
    public static string? PageContext(string ctlOutput, string windowTitle)
    {
        try
        {
            var tab = JsonDocument.Parse(ctlOutput.Trim()).RootElement;
            string Get(string k) => tab.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";
            var (title, text) = (Get("title"), Get("text"));
            // The window's title is the tab's, then " - " and the browser's
            // name: anything looser ("Home" inside "Home Assistant") could be
            // another window's tab.
            var front = windowTitle == title
                || windowTitle.StartsWith(title + " - ", StringComparison.Ordinal)
                || windowTitle.StartsWith(title + " \u2014 ", StringComparison.Ordinal);
            if (text.Length == 0 || title.Length == 0 || !front)
                return null;
            var sel = Get("selection");
            return $"The page in front (read from the browser when asked):\nTitle: {title}\nAddress: {Get("url")}\n"
                + (sel.Length > 0 ? $"Selected: {sel}\n" : "") + $"Text:\n{text}";
        }
        catch (JsonException)
        {
            return null;
        }
    }
}
