using System.Diagnostics;
using FridayTray.Core;
using Microsoft.Win32;

namespace FridayTray;

/// <summary>Finds the browsers on this PC and connects FRIDAY's extension to one (Windows side of <see cref="BrowserLink"/>).</summary>
internal static class BrowserSetup
{
    /// <summary>The browser's program, from Windows' App Paths, or null when it isn't installed.</summary>
    public static string? ExePath(KnownBrowser b)
    {
        foreach (var root in new[] { Registry.CurrentUser, Registry.LocalMachine })
        {
            using var key = root.OpenSubKey($@"Software\Microsoft\Windows\CurrentVersion\App Paths\{b.Exe}");
            if (key?.GetValue(null) is string path && File.Exists(path.Trim('"')))
                return path.Trim('"');
        }
        return null;
    }

    public static IEnumerable<KnownBrowser> Installed() => BrowserLink.All.Where(b => ExePath(b) is not null);

    public static string ExtensionDir(Install install) => Path.Combine(install.Dir, "browser-extension");

    /// <summary>Whether this browser already has FRIDAY's host registered.</summary>
    public static bool Connected(KnownBrowser b)
    {
        using var key = Registry.CurrentUser.OpenSubKey($@"{b.HostKeys[0]}\{BrowserLink.HostName}");
        return key?.GetValue(null) is string m && File.Exists(m);
    }

    /// <summary>
    /// Registers FRIDAY's host for the browser (a manifest in FRIDAY's config
    /// folder, named under HKCU), then opens its extensions page with the
    /// extension's folder on the clipboard. Returns what to do there.
    /// </summary>
    public static string Connect(KnownBrowser b, Install install)
    {
        var dir = Path.Combine(install.Paths.ConfigDir, "browser");
        Directory.CreateDirectory(dir);
        var manifest = Path.Combine(dir, $"{BrowserLink.HostName}.{(b.Firefox ? "firefox" : "chromium")}.json");
        File.WriteAllText(manifest, BrowserLink.Manifest(install.Ctl, b.Firefox));
        foreach (var hostKey in b.HostKeys)
        {
            using var key = Registry.CurrentUser.CreateSubKey($@"{hostKey}\{BrowserLink.HostName}");
            key.SetValue(null, manifest);
        }
        var ext = ExtensionDir(install);
        Selection.TrySetText(b.Firefox ? Path.Combine(ext, "manifest.json") : ext);
        if (ExePath(b) is { } exe)
        {
            try { Process.Start(new ProcessStartInfo(exe, b.ExtensionsPage) { UseShellExecute = false }); }
            catch (System.ComponentModel.Win32Exception) { }
        }
        return BrowserLink.Steps(b, ext);
    }
}
